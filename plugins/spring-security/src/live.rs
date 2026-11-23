//! Current-text checks and explicit XML Ant / Java factory facts; discovery is lifecycle-only.
use super::*;
use openidea_plugin::diagnostics::DiagnosticsProvider;
use openidea_plugin::hover::{Hover, HoverProvider};
use openidea_plugin::inlay_hints::{InlayHint, InlayHints};
use openidea_plugin::{Event, EventKind};
use std::time::{Duration, Instant};
const MAX_TEXT: usize = 128 << 10;
impl SecurityTools {
    pub(crate) fn activate_live(&mut self) {
        self.watching = host::subscribe_paths("files-changed", &beans::GLOBS).is_ok();
        if matches!(host::subscribe(EventKind::FileOpened), Err(Error::Denied(_))) {
            ui_kit::denied::notify_once(&self.events_notice, "Missing-bean checks", "events", "Spring Security Tools");
        }
        let _ = host::subscribe(EventKind::FileSaved);
        let deadline = Instant::now() + spring_text::index::COMMAND_BUDGET;
        for project in host::projects().unwrap_or_default() {
            let budget = deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(250));
            if budget.is_zero() {
                break;
            }
            self.beans.get(&project, budget);
            self.mappings.get(&project, deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(250)));
        }
    }
    pub(crate) fn live_events(&mut self, events: &[Event]) {
        let deadline = Instant::now() + spring_text::index::COMMAND_BUDGET;
        self.beans.on_events(events, deadline);
        self.mappings.on_events(events, deadline);
    }
}
fn supported(doc: &Document) -> bool {
    doc.text.len() <= MAX_TEXT
        && matches!(
            (doc.language.as_str(), doc.path.rsplit('.').next()),
            ("java", Some("java")) | ("kotlin", Some("kt")) | ("xml", Some("xml"))
        )
}
// Settings are read once per request, including XML groups previously emitted
// independently of the Java matcher switch. Defaults preserve existing checks.
struct DiagnosticSettings {
    migration: bool,
    roles: bool,
    passwords: bool,
    expressions: bool,
    xml: bool,
    matchers: bool,
}
impl DiagnosticSettings {
    fn load() -> Self {
        let on = |key| host::setting_bool(key).unwrap_or(true);
        Self {
            migration: on("springSecurity.checkMigration"),
            roles: on("springSecurity.checkRolePrefixes"),
            passwords: on("springSecurity.checkPasswords"),
            expressions: on("springSecurity.checkExpressions"),
            xml: on("springSecurity.checkXmlAttributes"),
            matchers: on("springSecurity.checkMatchers"),
        }
    }
    fn allows(&self, code: Option<&str>) -> bool {
        match code {
            Some("web-security-configurer-adapter" | "enable-global-method-security" | "authorize-requests" | "removed-request-matcher" | "global-method-security" | "use-expressions-false") => self.migration,
            Some("role-prefix") => self.roles,
            Some("no-op-password-encoder" | "plain-text-password") => self.passwords,
            Some("unbalanced-expression" | "empty-security-function" | "unknown-security-function" | "unknown-method-parameter") => self.expressions,
            Some("missing-attribute" | "invalid-attribute-value") => self.xml,
            Some("duplicate-pattern" | "unreachable-patterns" | "matcher-order" | "matchers-after-any-request") => self.matchers,
            _ => true,
        }
    }
}
impl DiagnosticsProvider for SecurityTools {
    fn diagnostics(&mut self, doc: &Document) -> Vec<Diagnostic> {
        if !supported(doc) {
            return Vec::new();
        }
        let mut found: Vec<_> = problems(&doc.path, &doc.text)
            .into_iter()
            .map(|p| p.diagnostic)
            .collect();
        found.extend(self.reference_findings(doc));
        found.extend(super::path_matcher::diagnostics(doc));
        let settings = DiagnosticSettings::load();
        found.retain(|d| settings.allows(d.code.as_deref()));
        found.truncate(500);
        found
    }
}
struct Matcher {
    start: usize,
    end: usize,
    path: String,
    method: Option<String>,
    strategy: &'static str,
    api: Option<&'static str>,
}
// Modern requestMatchers chooses its implementation from framework/classpath context.
// Explicit XML Ant and admitted Java PathPattern factories identify source contracts.
fn matchers(doc: &Document) -> Vec<Matcher> {
    if !supported(doc) { return Vec::new(); }
    if doc.language == "java" {
        let mut declared:Vec<_>=super::path_matcher::declarations(doc).into_iter().filter(|d| d.path.starts_with('/'))
            .map(|d| Matcher {start:d.from,end:d.to,path:d.path,method:d.method,strategy:"PathPattern",api:Some(d.api)}).collect();
        if host::setting_bool("springSecurity.checkMatchers").unwrap_or(true) {
            declared.extend(crate::modern_matcher::declarations(doc).into_iter().map(|d|Matcher{start:d.from,end:d.to,path:d.value,method:d.method,strategy:"Unresolved",api:Some("source-owned HttpSecurity authorizeHttpRequests lambda; String-pattern requestMatchers overload")}));
        }
        return declared;
    }
    if doc.language != "xml" || !host::setting_bool("springSecurity.checkMatchers").unwrap_or(true)
        || !spring_text::xml::analysis_valid(&doc.text) {
        return Vec::new();
    }
    let elements = xml::elements(&doc.text);
    elements
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            if e.local() != "intercept-url" || xml::namespace(&elements, i) != Some(SECURITY_NAMESPACE) {
                return None;
            }
            let parent_index = e.parent?;
            let parent = elements.get(parent_index)?;
            if xml::namespace(&elements, parent_index) != Some(SECURITY_NAMESPACE) {
                return None;
            }
            if parent.local() != "http"
                || parent.value("request-matcher") != Some("ant")
                || parent.attr("request-matcher-ref").is_some()
                || parent.attr("servlet-path").is_some()
                || e.attr("request-matcher-ref").is_some()
                || e.attr("servlet-path").is_some()
            {
                return None;
            }
            let pattern = e.attr("pattern")?;
            if !pattern.value.starts_with('/')
                || pattern.value.len() > 256
                || pattern.value.contains(['*', '?', '{', '}', '$', '\\', '`', '&', '<', '>'])
                || pattern.value.chars().any(char::is_control)
            {
                return None;
            }
            let method = e.value("method").map(str::to_owned);
            if method.as_deref().is_some_and(|m| !matches!(m, "GET" | "POST" | "PUT" | "DELETE" | "HEAD" | "OPTIONS" | "TRACE" | "PATCH")) {
                return None;
            }
            Some(Matcher { start: pattern.value_at, end: pattern.value_end, path: pattern.value.clone(), method, strategy:"Ant", api:None })
        })
        .take(200)
        .collect()
}
impl HoverProvider for SecurityTools {
    fn hover(&mut self, doc: &Document, at: Position) -> Option<Hover> {
        if let Some(hover) = crate::ownership::hover(doc,at) {return Some(hover);}
        if !host::setting_bool("springSecurity.matcherHover").unwrap_or(true) { return None; }
        let off = offset(&doc.text, at)?;
        let m = matchers(doc).into_iter().find(|m| m.start <= off && off < m.end)?;
        let title = if m.strategy == "Unresolved" {
            "Declared request pattern; matcher strategy unresolved".to_owned()
        } else {
            format!("Explicit {} request matcher", m.strategy)
        };
        let method = m.method.as_deref().unwrap_or("any method (no specific method declared)");
        let source = m.api.unwrap_or("explicit XML Ant declaration");
        Some(Hover::new(format!("{title}\n\nLiteral path: `{}`\n\nHTTP method: {method}.\n\nSource contract: {source}.\n\nRead from the source only; the runtime policy may differ.", m.path)).range(range(&doc.text, m.start, m.end)))
    }
}

impl InlayHints for SecurityTools {
    fn inlay_hints(&mut self, doc: &Document, requested: Range) -> Vec<InlayHint> {
        if !host::setting_bool("springSecurity.matcherInlays").unwrap_or(true) {
            return Vec::new();
        }
        matchers(doc)
            .into_iter()
            .filter_map(|m| {
                let end = range(&doc.text, m.start, m.end).end;
                if end < requested.start || end > requested.end {
                    return None;
                }
                let label = match (m.strategy, m.method.as_deref()) {
                    ("Unresolved", method) => method?.to_owned(),
                    (strategy, method) => format!("{strategy} · {}", method.unwrap_or("any method")),
                };
                Some(InlayHint::new(end, label).padding_left())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    fn doc(text: &str) -> Document {
        Document { path: "/project/security.xml".into(), language: "xml".into(), version: 7, text: text.into() }
    }
    fn xml(attrs: &str, pattern: &str) -> String {
        format!(
            r#"<s:http xmlns:s="{SECURITY_NAMESPACE}" {attrs}><s:intercept-url pattern="{pattern}" method="GET" access="permitAll"/></s:http>"#
        )
    }
    fn all() -> Range {
        Range::new(Position::new(0, 0), Position::new(u32::MAX, u32::MAX))
    }
    fn java_doc(expression: &str) -> Document {
        Document {path:"/project/Security.java".into(),language:"java".into(),version:21,
            text:format!("import org.springframework.security.web.servlet.util.matcher.PathPatternRequestMatcher;\r\nclass S {{Object f() {{ /* 😀 retained */ return {expression};}}}}")}
    }
    #[test]
    fn missing_bean_checks_notice_shown_once() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        let mut tools = SecurityTools::default();
        tools.activate_live();
        tools.activate_live();
        let notices: Vec<_> = host.notifications().into_iter().filter(|n| n.body.contains("needs the events permission")).collect();
        assert_eq!(notices.len(), 1);
        assert!(notices[0].body.contains("Settings › Plugins › Spring Security Tools"));
    }
    #[test]
    fn explicit_java_factory_hover_and_inlays_use_current_utf16_source_and_method() {
        let h=MockHost::install(); let mut tools=SecurityTools::default();
        for (expression,method,api) in [
            ("PathPatternRequestMatcher.withDefaults().matcher(\"/api/*\")","any method","6.5+"),
            ("PathPatternRequestMatcher.pathPattern(org.springframework.http.HttpMethod.GET, \"/api/*\")","GET","7.0+"),
        ] {
            let d=java_doc(expression); let start=d.text.find("/api/*").unwrap();
            let at=openidea_plugin::text::position(&d.text,start);
            let hover=tools.hover(&d,at).unwrap();
            assert_eq!(hover.range.unwrap(),range(&d.text,start,start+6));
            for claim in ["Explicit PathPattern",api,"Read from the source only"] {assert!(hover.markdown.contains(claim));}
            let hints=tools.inlay_hints(&d,all()); assert_eq!(hints.len(),1);
            assert_eq!(hints[0].label,format!("PathPattern · {method}"));
            assert_eq!(hints[0].position,openidea_plugin::text::position(&d.text,start+6));
            assert!(tools.inlay_hints(&d,Range::new(at,at)).is_empty());
        }
        assert!(h.process_runs().is_empty());assert_eq!(h.fetches(),0);
    }
    #[test]
    fn matcher_feature_switches_are_independent_of_path_diagnostics_and_each_other() {
        let h=MockHost::install(); let mut tools=SecurityTools::default();
        let d=java_doc("PathPatternRequestMatcher.pathPattern(\"/api\")");
        let at=openidea_plugin::text::position(&d.text,d.text.find("/api").unwrap());
        h.set_setting("springSecurity.checkPathPatterns","false");
        assert!(tools.hover(&d,at).is_some());assert_eq!(tools.inlay_hints(&d,all()).len(),1);
        h.set_setting("springSecurity.matcherHover","false");
        assert!(tools.hover(&d,at).is_none());assert_eq!(tools.inlay_hints(&d,all()).len(),1);
        h.set_setting("springSecurity.matcherInlays","false");assert!(tools.inlay_hints(&d,all()).is_empty());
        h.set_setting("springSecurity.matcherHover","true");assert!(tools.hover(&d,at).is_some());
    }
    #[test]
    fn unknown_or_invalid_java_factories_do_not_supply_successful_matcher_facts() {
        let _h=MockHost::install();let mut tools=SecurityTools::default();
        for expression in [
            "PathPatternRequestMatcher.pathPattern(\"api\")",
            "PathPatternRequestMatcher.pathPattern(\"/api/**/tail\")",
            "PathPatternRequestMatcher.pathPattern(\"/api\" + dynamic)",
            "PathPatternRequestMatcher.pathPattern(method, \"/api\")",
            "http.requestMatchers(\"/api\")",
        ] {
            let d=java_doc(expression);assert!(tools.inlay_hints(&d,all()).is_empty(),"{expression}");
            let at=openidea_plugin::text::position(&d.text,d.text.rfind("api").unwrap());assert!(tools.hover(&d,at).is_none());
        }
        let mut d=java_doc("PathPatternRequestMatcher.pathPattern(\"/api\")");
        d.text=d.text.replace("org.springframework.security.web.servlet.util.matcher","foreign");
        assert!(tools.inlay_hints(&d,all()).is_empty());
    }
    #[test]
    fn unsaved_xml_and_fixed_version_replace_provider_results_without_marker_publication() {
        let host = MockHost::install();
        let mut tools = SecurityTools::default();
        let mut d = doc(&xml("request-matcher=\"ant\"", "/x"));
        d.text = d.text.replace("method=\"GET\"", "method=\"BAD\"");
        host.open_editor(&d.path, &d.language, &d.text, &[]);
        host.refresh_diagnostics(&mut tools);
        assert!(!host.provider_diagnostics()[&d.path].is_empty());
        d.version += 1;
        d.text = xml("request-matcher=\"ant\"", "/x");
        host.open_editor(&d.path, &d.language, &d.text, &[]);
        host.refresh_diagnostics(&mut tools);
        assert!(host.provider_diagnostics().get(&d.path).is_none_or(Vec::is_empty));
        tools.line_markers(&d);
        assert!(host.diagnostics().is_empty());
    }
    #[test]
    fn explicit_ant_hints_have_exact_utf16_ranges_and_no_policy_claim() {
        let _host = MockHost::install();
        let mut tools = SecurityTools::default();
        let d = doc(&format!("<!-- 😀 -->{}", xml("request-matcher=\"ant\"", "/é")));
        let at = openidea_plugin::text::position(&d.text, d.text.find("/é").unwrap());
        let hover = tools.hover(&d, at).unwrap();
        assert_eq!(hover.range.unwrap().start, at);
        let hints = tools.inlay_hints(&d, all());
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].label, "Ant · GET");
        assert_eq!(hints[0].position, openidea_plugin::text::position(&d.text, d.text.find("/é").unwrap() + "/é".len()));
        assert!(tools.inlay_hints(&d, Range::new(Position::new(0, 0), at)).is_empty());
    }
    #[test]
    fn unknown_dynamic_custom_namespace_and_disabled_matchers_are_quiet() {
        let host = MockHost::install();
        let mut tools = SecurityTools::default();
        for text in [
            xml("", "/x"),
            xml("request-matcher=\"mvc\"", "/x"),
            xml("request-matcher=\"regex\"", "/x"),
            xml("request-matcher=\"ant\" request-matcher-ref=\"custom\"", "/x"),
            xml("request-matcher=\"ant\"", "/${path}"),
            xml("request-matcher=\"ant\"", "/**"),
            xml("request-matcher=\"ant\"", "/x").replace(SECURITY_NAMESPACE, "urn:other"),
            format!(
                r#"<http xmlns="urn:other" request-matcher="ant"><s:intercept-url xmlns:s="{SECURITY_NAMESPACE}" pattern="/x"/></http>"#
            ),
        ] {
            let d = doc(&text);
            assert!(tools.inlay_hints(&d, all()).is_empty(), "{text}");
        }
        host.set_setting("springSecurity.checkMatchers", "false");
        assert!(tools.inlay_hints(&doc(&xml("request-matcher=\"ant\"", "/x")), all()).is_empty());
    }
    #[test]
    fn java_feature_switches_preserve_other_findings_and_current_unsaved_results() {
        let h = MockHost::install();
        let mut tools = SecurityTools::default();
        let mut d = Document {path:"/project/S.java".into(),language:"java".into(),version:5,text:r#"import org.springframework.security.config.annotation.web.builders.HttpSecurity;
class S extends WebSecurityConfigurerAdapter {
 @EnableGlobalMethodSecurity void f(HttpSecurity http) {
  http.authorizeRequests().anyRequest().authenticated().antMatchers("/api");
  http.hasRole("ROLE_ADMIN"); NoOpPasswordEncoder.getInstance(); user.password("{noop}secret");
 }
}"#.into()};
        let codes = |tools:&mut SecurityTools,d:&Document| tools.diagnostics(d).into_iter().filter_map(|d|d.code).collect::<BTreeSet<_>>();
        let before = codes(&mut tools,&d);
        for code in ["web-security-configurer-adapter","enable-global-method-security","authorize-requests","removed-request-matcher","role-prefix","no-op-password-encoder","plain-text-password","matchers-after-any-request"] { assert!(before.contains(code),"{code}: {before:?}"); }
        h.set_setting("springSecurity.checkMigration","false");
        let without_migration = codes(&mut tools,&d);
        assert!(without_migration.contains("role-prefix") && without_migration.contains("plain-text-password") && without_migration.contains("matchers-after-any-request"));
        for code in ["web-security-configurer-adapter","enable-global-method-security","authorize-requests","removed-request-matcher"] {assert!(!without_migration.contains(code));}
        h.set_setting("springSecurity.checkRolePrefixes","false");
        assert!(!codes(&mut tools,&d).contains("role-prefix"));
        assert!(codes(&mut tools,&d).contains("plain-text-password"));
        h.set_setting("springSecurity.checkPasswords","false");
        let remaining = codes(&mut tools,&d);
        assert!(!remaining.contains("plain-text-password") && !remaining.contains("no-op-password-encoder"));
        assert!(remaining.contains("matchers-after-any-request"));
        h.set_setting("springSecurity.checkMatchers","false");
        assert!(tools.diagnostics(&d).is_empty());
        h.set_setting("springSecurity.checkRolePrefixes","true");
        assert_eq!(codes(&mut tools,&d), BTreeSet::from(["role-prefix".to_owned()]));
        d.version+=1; d.text=d.text.replace("ROLE_ADMIN","ADMIN");
        assert!(tools.diagnostics(&d).is_empty());
    }
    #[test]
    fn kotlin_source_advisories_obey_the_same_independent_feature_switches() {
        let h=MockHost::install();
        let mut tools=SecurityTools::default();
        let d=Document{path:"/project/S.kt".into(),language:"kotlin".into(),version:7,text:r#"import org.springframework.security.config.annotation.web.builders.HttpSecurity
class S {
 fun filter(http: HttpSecurity) {
  http.antMatchers("/api")
  http.hasRole("ROLE_USER")
  user.password("{noop}example")
 }
}"#.into()};
        let codes=|tools:&mut SecurityTools|tools.diagnostics(&d).into_iter().filter_map(|d|d.code).collect::<BTreeSet<_>>();
        assert_eq!(codes(&mut tools),BTreeSet::from(["removed-request-matcher".to_owned(),"role-prefix".to_owned(),"plain-text-password".to_owned()]));
        h.set_setting("springSecurity.checkMigration","false");
        h.set_setting("springSecurity.checkPasswords","false");
        assert_eq!(codes(&mut tools),BTreeSet::from(["role-prefix".to_owned()]));
        h.set_setting("springSecurity.checkRolePrefixes","false");
        assert!(tools.diagnostics(&d).is_empty());
        h.set_setting("springSecurity.checkMigration","true");
        assert_eq!(codes(&mut tools),BTreeSet::from(["removed-request-matcher".to_owned()]));
    }
    #[test]
    fn xml_feature_switches_keep_schema_expression_matcher_and_bean_groups_independent() {
        let h=MockHost::install();
        h.add_file("A.java","class A {}");
        let mut tools=SecurityTools::default(); tools.activate_live();
        let d=doc(&format!(r#"<s:beans xmlns:s="{SECURITY_NAMESPACE}"><s:global-method-security/><s:http create-session="invalid" entry-point-ref="missing" use-expressions="false"><s:intercept-url pattern="/**"/><s:intercept-url pattern="/api"/><s:intercept-url pattern="/api"/></s:http><s:http><s:intercept-url access="hasRole() and hasUnknown()"/></s:http></s:beans>"#));
        let codes=|tools:&mut SecurityTools|tools.diagnostics(&d).into_iter().filter_map(|d|d.code).collect::<BTreeSet<_>>();
        let before=codes(&mut tools);
        for code in ["global-method-security","use-expressions-false","invalid-attribute-value","missing-attribute","empty-security-function","unknown-security-function","duplicate-pattern","unreachable-patterns","unresolved-reference"] {assert!(before.contains(code),"{code}: {before:?}");}
        h.set_setting("springSecurity.checkXmlAttributes","false");
        let after=codes(&mut tools);
        assert!(!after.contains("invalid-attribute-value")&&!after.contains("missing-attribute"));
        assert!(after.contains("unresolved-reference")&&after.contains("unknown-security-function"));
        h.set_setting("springSecurity.checkExpressions","false");
        let after=codes(&mut tools);
        assert!(!after.contains("empty-security-function")&&!after.contains("unknown-security-function"));
        assert!(after.contains("duplicate-pattern")&&after.contains("unresolved-reference"));
        h.set_setting("springSecurity.checkMatchers","false");
        let after=codes(&mut tools);
        assert!(!after.contains("duplicate-pattern")&&!after.contains("unreachable-patterns"));
        assert!(after.contains("global-method-security")&&after.contains("unresolved-reference"));
        h.set_setting("springSecurity.checkMigration","false");
        assert_eq!(codes(&mut tools),BTreeSet::from(["unresolved-reference".to_owned()]));
        h.set_setting("springSecurity.checkBeanReferences","false");
        assert!(tools.diagnostics(&d).is_empty());
        h.set_setting("springSecurity.checkXmlAttributes","true");
        assert_eq!(codes(&mut tools),BTreeSet::from(["invalid-attribute-value".to_owned(),"missing-attribute".to_owned()]));
    }
    #[test]
    fn invalid_xml_never_becomes_explicit_ant_hover_or_inlays() {
        let _h=MockHost::install(); let mut tools=SecurityTools::default();
        let valid=xml("request-matcher=\"ant\"","/api");
        for text in [
            valid.replace("xmlns:s=", "xmlns:xml=").replace("<s:","<xml:").replace("</s:","</xml:"),
            valid.replace("<s:http ","<s:http xmlns:xml='urn:foreign' "),
            valid.replace("</s:http>",""),
            format!("{valid}<extra/>"),
            format!("{valid}trailing"),
            valid.replace("<s:intercept-url", "<s:intercept-url unbound:attr='x'"),
        ] {
            let d=doc(&text); let at=openidea_plugin::text::position(&d.text,d.text.find("/api").unwrap());
            assert!(tools.hover(&d,at).is_none(),"hover accepted {}",d.text);
            assert!(tools.inlay_hints(&d,all()).is_empty(),"inlays accepted {}",d.text);
        }
        let d=doc(&valid);let at=openidea_plugin::text::position(&d.text,d.text.find("/api").unwrap());
        assert!(tools.hover(&d,at).is_some()); assert_eq!(tools.inlay_hints(&d,all()).len(),1);
    }
    #[test]
    fn matcher_hover_ranges_are_half_open_for_xml_explicit_and_modern_java() {
        let _h=MockHost::install(); let mut tools=SecurityTools::default();
        let modern=Document{path:"/project/S.java".into(),language:"java".into(),version:1,text:"import org.springframework.security.config.annotation.web.builders.HttpSecurity; class S {void f(HttpSecurity http){http.authorizeHttpRequests(a -> a.requestMatchers(\"/api\"));}}".into()};
        for d in [doc(&xml("request-matcher=\"ant\"","/api")),java_doc("PathPatternRequestMatcher.pathPattern(\"/api\")"),modern] {
            let from=d.text.find("/api").unwrap();
            assert!(tools.hover(&d,openidea_plugin::text::position(&d.text,from)).is_some());
            assert!(tools.hover(&d,openidea_plugin::text::position(&d.text,from+3)).is_some());
            assert!(tools.hover(&d,openidea_plugin::text::position(&d.text,from+4)).is_none(),"end accepted {}",d.text);
        }
    }
    #[test]
    fn cold_and_oversized_caches_do_not_claim_missing_beans() {
        let host = MockHost::install();
        let mut tools = SecurityTools::default();
        let d = doc(&format!(r#"<s:http xmlns:s="{SECURITY_NAMESPACE}" entry-point-ref="external"/>"#));
        assert!(tools.diagnostics(&d).iter().all(|d| d.code.as_deref() != Some("unresolved-bean-reference")));
        host.add_file("Huge.java", &" ".repeat(MAX_TEXT + 1));
        tools.activate_live();
        assert!(tools.reference_findings(&d).is_empty());
        let mut huge = d.clone();
        huge.text = " ".repeat(MAX_TEXT + 1);
        assert!(tools.diagnostics(&huge).is_empty());
    }
    #[test]
    fn unsaved_bean_definition_replaces_cached_current_source() {
        let host = MockHost::install();
        host.add_file("S.java", "class S {}");
        let mut tools = SecurityTools::default();
        tools.activate_live();
        let mut d = Document {
            path: "/project/S.java".into(),
            language: "java".into(),
            version: 2,
            text: r#"@Service class S { @PreAuthorize("@local.ok()") void f() {} }"#.into(),
        };
        assert!(tools.diagnostics(&d).iter().any(|d| d.code.as_deref() == Some("unresolved-bean-reference")));
        d.version += 1;
        d.text = r#"@Service class S { @Bean Object local() {return null;} @PreAuthorize("@local.ok()") void f() {} }"#.into();
        assert!(tools.diagnostics(&d).iter().all(|d| d.code.as_deref() != Some("unresolved-bean-reference")));
        d.version += 1;
        d.text = r#"@Service class S { @PreAuthorize("@local.ok()") void f() {} }"#.into();
        assert!(tools.diagnostics(&d).iter().any(|d| d.code.as_deref() == Some("unresolved-bean-reference")));
    }
    #[test]
    fn matcher_inlay_hidden_when_unresolved() {
        let _h = MockHost::install();
        let mut tools = SecurityTools::default();
        let modern = Document {
            path: "/project/S.java".into(),
            language: "java".into(),
            version: 1,
            text: "import org.springframework.security.config.annotation.web.builders.HttpSecurity; class S {void f(HttpSecurity http){http.authorizeHttpRequests(a -> a.requestMatchers(\"/api\"));}}".into(),
        };
        assert!(tools.inlay_hints(&modern, all()).is_empty());
        let with_method = Document {
            text: format!("import org.springframework.http.HttpMethod;\n{}", modern.text.replace("requestMatchers(\"/api\")", "requestMatchers(HttpMethod.GET, \"/api\")")),
            ..modern
        };
        let hints = tools.inlay_hints(&with_method, all());
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].label, "GET");
    }
    #[test]
    fn matcher_inlay_shows_http_method() {
        let _h = MockHost::install();
        let mut tools = SecurityTools::default();
        let hints = tools.inlay_hints(&doc(&xml("request-matcher=\"ant\"", "/api")), all());
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].label, "Ant · GET");
    }
    #[test]
    fn unknown_bean_info_survives_ttl() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Authz.java", "@Component class Authz {}");
        let src = "@Service\nclass S {\n    @PreAuthorize(\"@ghost.ok()\")\n    void m() {}\n}\n";
        let d = Document { path: "/project/src/main/java/com/acme/S.java".into(), language: "java".into(), version: 1, text: src.into() };
        let mut tools = SecurityTools::default();
        tools.activate_live();
        let unknown = |tools: &mut SecurityTools| tools.diagnostics(&d).iter().any(|x| x.code.as_deref() == Some("unresolved-bean-reference"));
        assert!(unknown(&mut tools));
        tools.beans.for_cached_document(&d.path, &d.text).unwrap().mark_stale();
        assert!(unknown(&mut tools));
    }
    #[test]
    fn line_markers_advance_cold_index() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Authz.java", "@Component class Authz {}");
        let d = Document { path: "/project/src/main/java/com/acme/S.java".into(), language: "java".into(), version: 1, text: "class S {}".into() };
        let mut tools = SecurityTools::default();
        tools.line_markers(&d);
        let index = tools.beans.cached(&d.path).unwrap();
        assert!(index.fresh() && index.iter().any(|(p, _)| p.ends_with("Authz.java")));
    }
    #[test]
    fn bean_name_set_is_cached_per_generation() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Authz.java", "@Component class Authz {}");
        let d = Document { path: "/project/src/main/java/com/acme/S.java".into(), language: "java".into(), version: 1, text: "@Component class S {}".into() };
        let mut tools = SecurityTools::default();
        tools.activate_live();
        let first = tools.project_bean_names(&d, true).unwrap();
        let second = tools.project_bean_names(&d, true).unwrap();
        assert!(Rc::ptr_eq(&first, &second));
    }
    #[test]
    fn doctype_xml_still_gets_hover_and_inlays() {
        let _h = MockHost::install();
        let mut tools = SecurityTools::default();
        let d = doc(&format!("<!DOCTYPE beans>{}", xml("request-matcher=\"ant\"", "/api")));
        let at = openidea_plugin::text::position(&d.text, d.text.find("/api").unwrap());
        assert!(tools.hover(&d, at).is_some());
        assert_eq!(tools.inlay_hints(&d, all()).len(), 1);
    }
}
