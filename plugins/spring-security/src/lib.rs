//! Spring Security Tools for OpenIDEA: flags configuration that Spring
//! Security 6 removed or deprecated (with quick fixes where the change is
//! mechanical), redundant `ROLE_` prefixes and plain-text password encoders,
//! and completes SpEL in method security annotations. Text-based, Java only.

use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{Diagnostic, Error, Invocation, Plugin, Position, Range, Severity, TextEdit, host, register_plugin};
use spring_text::index::{Indexes, PROVIDER_BUDGET};
use spring_text::java::{self, Annotation, JavaFile};
use spring_text::range;
use std::collections::BTreeSet;

const METHOD_SECURITY: [&str; 4] = ["PreAuthorize", "PostAuthorize", "PreFilter", "PostFilter"];
const GLOBAL_METHOD_SECURITY: &str =
    "org.springframework.security.config.annotation.method.configuration.EnableGlobalMethodSecurity";
const ROLE_PREFIX: &str = "ROLE_";
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

fn problems(text: &str) -> Vec<Problem> {
    let file = java::parse(text);
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
    for c in java::calls(text) {
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
        let found: Vec<Diagnostic> = problems(&doc.text).into_iter().map(|p| p.diagnostic).collect();
        let _ = host::publish_diagnostics(&doc.path, &found);
        Vec::new()
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        problems(&doc.text)
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
}
