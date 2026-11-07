//! References to beans in security configuration, checked against the beans
//! the project declares. Beans of libraries and auto-configuration are
//! invisible, so every finding is Info.

use spring_text::beans;
use spring_text::finding::Finding;
use spring_text::java::Annotation;
use spring_text::xml::{self, Element};
use spring_text::xml_check::{Rule, check as run_rules};
use spring_text::{decapitalize, parse_source};

use openidea_plugin::Severity;

const STEREOTYPES: [&str; 8] =
    ["Component", "Service", "Repository", "Controller", "RestController", "Configuration", "ControllerAdvice", "RestControllerAdvice"];
const LIBRARY_BEANS: [&str; 8] = [
    "authenticationManager",
    "userDetailsService",
    "passwordEncoder",
    "springSecurityFilterChain",
    "authenticationConfiguration",
    "methodSecurityExpressionHandler",
    "webSecurityExpressionHandler",
    "mvcHandlerMappingIntrospector",
];
const NAMESPACE: &str = "http://www.springframework.org/schema/security";

const HTTP_METHODS: &[&str] = &["GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS", "TRACE", "PATCH"];

const RULES: &[Rule] = &[
    Rule {
        enums: &[
            ("create-session", &["always", "never", "if_required", "stateless", "ifRequired"]),
            ("request-matcher", &["ant", "mvc", "regex", "ciRegex"]),
        ],
        refs: &["authentication-manager-ref", "entry-point-ref"],
        ..Rule::new("http")
    },
    Rule {
        required: &["pattern|request-matcher-ref"],
        enums: &[("requires-channel", &["any", "http", "https"]), ("method", HTTP_METHODS)],
        ..Rule::new("intercept-url")
    },
    Rule {
        refs: &["authentication-success-handler-ref", "authentication-failure-handler-ref", "authentication-details-source-ref"],
        ..Rule::new("form-login")
    },
    Rule { refs: &["entry-point-ref"], ..Rule::new("http-basic") },
    Rule { refs: &["user-service-ref"], ..Rule::new("authentication-provider") },
    Rule { required: &["name"], ..Rule::new("user") },
    Rule {
        enums: &[("session-fixation-protection", &["none", "newSession", "migrateSession", "changeSessionId"])],
        ..Rule::new("session-management")
    },
    Rule { refs: &["user-service-ref", "authentication-success-handler-ref"], ..Rule::new("remember-me") },
    Rule { refs: &["success-handler-ref"], ..Rule::new("logout") },
    Rule { required: &["ref|hash"], refs: &["ref"], ..Rule::new("password-encoder") },
    Rule { required: &["data-source-ref"], refs: &["data-source-ref"], ..Rule::new("jdbc-user-service") },
    Rule {
        enums: &[
            ("mode", &["proxy", "aspectj"]),
            ("pre-post-annotations", ENABLED),
            ("secured-annotations", ENABLED),
            ("jsr250-annotations", ENABLED),
            ("proxy-target-class", BOOLEAN),
        ],
        refs: &["access-decision-manager-ref", "authentication-manager-ref"],
        ..Rule::new("global-method-security")
    },
    Rule {
        enums: &[
            ("mode", &["proxy", "aspectj"]),
            ("pre-post-enabled", BOOLEAN),
            ("secured-enabled", BOOLEAN),
            ("jsr250-enabled", BOOLEAN),
        ],
        ..Rule::new("method-security")
    },
    Rule {
        enums: &[
            ("auto-config", BOOLEAN),
            ("once-per-request", BOOLEAN),
            ("disable-url-rewriting", BOOLEAN),
            ("servlet-api-provision", BOOLEAN),
            ("jaas-api-provision", BOOLEAN),
        ],
        refs: &["access-decision-manager-ref", "security-context-repository-ref", "request-cache-ref"],
        ..Rule::new("http")
    },
    Rule { enums: &[("always-use-default-target", BOOLEAN)], ..Rule::new("form-login") },
    Rule { enums: &[("invalidate-session", BOOLEAN)], ..Rule::new("logout") },
    Rule { refs: &["data-source-ref", "token-repository-ref", "services-ref"], ..Rule::new("remember-me") },
    Rule { enums: &[("disabled", BOOLEAN)], refs: &["token-repository-ref", "request-handler-ref"], ..Rule::new("csrf") },
    Rule { enums: &[("disabled", BOOLEAN), ("defaults-disabled", BOOLEAN)], ..Rule::new("headers") },
    Rule { refs: &["ref", "configuration-source-ref"], ..Rule::new("cors") },
    Rule { enums: &[("enabled", BOOLEAN)], ..Rule::new("anonymous") },
    Rule { required: &["ref"], refs: &["ref"], ..Rule::new("custom-filter") },
    Rule { refs: &["session-authentication-strategy-ref"], ..Rule::new("session-management") },
    Rule {
        enums: &[("error-if-maximum-exceeded", BOOLEAN)],
        refs: &["session-registry-ref"],
        ..Rule::new("concurrency-control")
    },
    Rule { enums: &[("erase-credentials", BOOLEAN)], ..Rule::new("authentication-manager") },
];

const BOOLEAN: &[&str] = &["true", "false"];
const ENABLED: &[&str] = &["enabled", "disabled"];

/// The bean names a source declares: stereotype classes, `@Bean` methods, repository interfaces and XML bean ids.
pub fn bean_names(path: &str, text: &str) -> Option<Vec<String>> {
    if beans::ignored(path) {
        return None;
    }
    let mut out = Vec::new();
    if path.ends_with(".xml") {
        out.extend(xml::bean_ids(&xml::elements(text)).into_iter().map(|a| a.value.clone()));
    } else {
        for ty in &parse_source(path, text).types {
            let explicit = |a: &Annotation| a.strings(&["value"]).first().map(|s| s.value.clone());
            if let Some(a) = ty.annotations.iter().find(|a| STEREOTYPES.contains(&a.name.as_str())) {
                out.push(explicit(a).unwrap_or_else(|| decapitalize(&ty.name)));
            }
            if ty.kind == "interface" && ty.extends.iter().any(|e| e.name.ends_with("Repository")) {
                out.push(decapitalize(&ty.name));
            }
            for m in ty.methods().filter(|m| m.annotation("Bean").is_some()) {
                let named: Vec<_> = m.annotation("Bean").map(|a| a.strings(&["name", "value"])).unwrap_or_default();
                out.extend(named.iter().map(|s| s.value.clone()));
                if named.is_empty() {
                    out.push(m.name.clone());
                }
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

fn known(name: &str, names: &dyn Fn(&str) -> bool) -> bool {
    beans::is_expression(name) || LIBRARY_BEANS.contains(&name) || beans::INFRASTRUCTURE.contains(&name) || names(name)
}

fn unresolved(at: usize, end: usize, name: &str) -> Finding {
    Finding::new(
        at,
        end,
        "unresolved-bean-reference",
        format!("No bean named \"{name}\" is declared in the project; it may come from a library or auto-configuration."),
    )
    .with_severity(Severity::Info)
}

/// `@name` bean references of a SpEL expression: byte offset and name.
fn spel_beans(expr: &str) -> Vec<(usize, &str)> {
    let bytes = expr.as_bytes();
    let mut out = Vec::new();
    for (i, _) in expr.match_indices('@') {
        let name_end = expr[i + 1..].find(|c: char| !(c.is_alphanumeric() || c == '_')).map_or(expr.len(), |e| i + 1 + e);
        let starts_word = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'.');
        let called = matches!(bytes.get(name_end), Some(b'.' | b'(' | b' ' | b')') | None);
        if starts_word && called && name_end > i + 1 {
            out.push((i + 1, &expr[i + 1..name_end]));
        }
    }
    out
}

/// References of annotations to beans: `@WithUserDetails(userDetailsServiceBeanName)` and `@bean` in security expressions.
pub fn source_references(path: &str, text: &str, is_bean: &dyn Fn(&str) -> bool) -> Vec<Finding> {
    let file = parse_source(path, text);
    let mut out = Vec::new();
    for a in file.types.iter().flat_map(|t| t.annotations.iter().chain(t.members.iter().flat_map(|m| &m.annotations))) {
        if a.name == "WithUserDetails" {
            for s in a.strings(&["userDetailsServiceBeanName"]) {
                if !known(&s.value, is_bean) {
                    out.push(unresolved(s.at, s.end, &s.value));
                }
            }
        }
        if crate::METHOD_SECURITY.contains(&a.name.as_str()) {
            for s in a.strings(&["value"]).into_iter().filter(|s| s.value.len() == s.end - s.at) {
                for (at, name) in spel_beans(&s.value) {
                    if !known(name, is_bean) {
                        out.push(unresolved(s.at + at, s.at + at + name.len(), name));
                    }
                }
            }
        }
    }
    out
}

/// Table checks of the security namespace; bean references only when `is_bean` is given.
pub fn xml_rules(text: &str, is_bean: Option<&dyn Fn(&str) -> bool>) -> Vec<Finding> {
    if !text.contains(NAMESPACE) {
        return Vec::new();
    }
    let all = xml::elements(text);
    let in_security: Vec<Element> =
        (0..all.len()).filter(|&i| xml::namespace(&all, i) == Some(NAMESPACE)).map(|i| all[i].clone()).collect();
    run_rules(&in_security, RULES, |name| is_bean.is_none_or(|f| known(name, f)))
        .into_iter()
        .map(|f| if f.code == "unresolved-reference" { f.with_severity(Severity::Info) } else { f })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_declared_bean_names() {
        let java = "@Service class UserLookup {}\n@Component(\"audit\") class A {}\n@Configuration class C { @Bean Foo foo() {} @Bean(\"bar\") Bar b() {} }\ninterface OrderRepository extends JpaRepository<O, Long> {}";
        assert_eq!(bean_names("A.java", java).unwrap(), ["userLookup", "audit", "c", "foo", "bar", "orderRepository"]);
        let xml = r#"<beans><bean id="x" class="A"/></beans>"#;
        assert_eq!(bean_names("c.xml", xml).unwrap(), ["x"]);
        assert!(bean_names("/p/target/c.xml", xml).is_none());
    }

    #[test]
    fn finds_spel_bean_references() {
        assert_eq!(spel_beans("@authz.check(#id) and hasRole('A@B') and x@y"), [(1, "authz")]);
        assert_eq!(spel_beans("!@a"), [(2, "a")]);
    }

    #[test]
    fn reports_unknown_beans_as_info() {
        let src = "@WithUserDetails(value = \"u\", userDetailsServiceBeanName = \"nope\")\nclass T {\n @PreAuthorize(\"@authz.ok() and @missing.ok()\") void m() {}\n @WithUserDetails(userDetailsServiceBeanName = \"authz\") void n() {}\n}";
        let is_bean = |n: &str| n == "authz";
        let found: Vec<_> = source_references("T.java", src, &is_bean).into_iter().map(|f| (src[f.start..f.end].to_owned(), f.severity)).collect();
        assert_eq!(found, [("nope".to_owned(), Severity::Info), ("missing".to_owned(), Severity::Info)]);
    }

    #[test]
    fn checks_security_xml_tables() {
        let xml = r#"<b:beans xmlns:b="http://www.springframework.org/schema/beans" xmlns="http://www.springframework.org/schema/security">
<http create-session="sometimes" authentication-manager-ref="am"><intercept-url access="x" method="FETCH"/><form-login authentication-success-handler-ref="h"/></http>
<authentication-manager><authentication-provider user-service-ref="svc"><user-service><user password="p"/></user-service></authentication-provider></authentication-manager>
<password-encoder/></b:beans>"#;
        let is_bean = |n: &str| n == "svc";
        let codes: Vec<_> = xml_rules(xml, Some(&is_bean)).into_iter().map(|f| (f.code, xml[f.start..f.end].to_owned())).collect();
        assert_eq!(
            codes,
            [
                ("invalid-attribute-value", "sometimes".into()),
                ("unresolved-reference", "am".into()),
                ("missing-attribute", "intercept-url".into()),
                ("invalid-attribute-value", "FETCH".into()),
                ("unresolved-reference", "h".into()),
                ("missing-attribute", "user".into()),
                ("missing-attribute", "password-encoder".into()),
            ]
        );
        assert_eq!(xml_rules(xml, None).len(), 5);
    }

    #[test]
    fn checks_the_wider_security_xml_tables() {
        let xml = r#"<b:beans xmlns:b="http://www.springframework.org/schema/beans" xmlns="http://www.springframework.org/schema/security">
<global-method-security pre-post-annotations="true" secured-annotations="enabled"/>
<http auto-config="yes"><csrf disabled="maybe"/><custom-filter after="BASIC_AUTH_FILTER"/><cors ref="corsSource"/></http>
<method-security pre-post-enabled="true"/></b:beans>"#;
        let none = |_: &str| false;
        let codes: Vec<_> = xml_rules(xml, Some(&none)).into_iter().map(|f| (f.code, xml[f.start..f.end].to_owned())).collect();
        assert_eq!(
            codes,
            [
                ("invalid-attribute-value", "true".into()),
                ("invalid-attribute-value", "yes".into()),
                ("invalid-attribute-value", "maybe".into()),
                ("missing-attribute", "custom-filter".into()),
                ("unresolved-reference", "corsSource".into()),
            ]
        );
    }
}
