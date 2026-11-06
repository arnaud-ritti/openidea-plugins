//! Attribute and bean reference checks of the `<websocket:...>` namespace of
//! Spring XML configuration, from a hard-coded table since no schema is at
//! hand. Beans of libraries are invisible, so references are reported as Info.

use openidea_plugin::Severity;
use spring_text::finding::Finding;
use spring_text::xml::{self, Element};
use spring_text::xml_check::{Rule, check as run};
use spring_text::{beans, decapitalize, parse_source};

const NAMESPACE: &str = "http://www.springframework.org/schema/websocket";
const COMPONENTS: [&str; 6] = ["Component", "Service", "Repository", "Controller", "RestController", "Configuration"];

const RULES: &[Rule] = &[
    Rule { required: &["path"], ..Rule::new("stomp-endpoint") },
    Rule { required: &["path", "handler"], refs: &["handler"], ..Rule::new("mapping") },
    Rule { required: &["prefix"], ..Rule::new("simple-broker") },
    Rule { required: &["prefix"], ..Rule::new("stomp-broker-relay") },
    Rule { required: &["ref"], refs: &["ref"], ..Rule::new("handshake-handler") },
];

/// The bean names a source declares: component classes, `@Bean` methods and XML bean ids.
pub fn bean_names(path: &str, text: &str) -> Option<Vec<String>> {
    if beans::ignored(path) {
        return None;
    }
    let mut out = Vec::new();
    if path.ends_with(".xml") {
        out.extend(xml::bean_ids(&xml::elements(text)).into_iter().map(|a| a.value.clone()));
    } else {
        for ty in &parse_source(path, text).types {
            if let Some(a) = ty.annotations.iter().find(|a| COMPONENTS.contains(&a.name.as_str())) {
                out.push(a.strings(&["value"]).first().map_or_else(|| decapitalize(&ty.name), |s| s.value.clone()));
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

/// Findings of the websocket elements; bean references are checked only when `is_bean` is given.
pub fn check(text: &str, is_bean: Option<&dyn Fn(&str) -> bool>) -> Vec<Finding> {
    if !text.contains(NAMESPACE) {
        return Vec::new();
    }
    let all = xml::elements(text);
    let in_namespace: Vec<Element> = (0..all.len()).filter(|&i| xml::namespace(&all, i) == Some(NAMESPACE)).map(|i| all[i].clone()).collect();
    run(&in_namespace, RULES, |name| {
        is_bean.is_none_or(|f| f(name)) || beans::is_expression(name) || beans::INFRASTRUCTURE.contains(&name)
    })
    .into_iter()
    .map(|f| if f.code == "unresolved-reference" { f.with_severity(Severity::Info) } else { f })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<beans xmlns="http://www.springframework.org/schema/beans" xmlns:websocket="http://www.springframework.org/schema/websocket">
  <websocket:message-broker><websocket:stomp-endpoint/><websocket:simple-broker prefix="/topic"/></websocket:message-broker>
  <websocket:handlers><websocket:mapping path="/echo" handler="echoHandler"/><websocket:mapping handler="ghost"/></websocket:handlers>
  <bean id="echoHandler" class="com.acme.Echo"/>
</beans>"#;

    #[test]
    fn collects_bean_names() {
        assert_eq!(bean_names("c.xml", XML).unwrap(), ["echoHandler"]);
        assert_eq!(bean_names("A.java", "@Component class Chat {} @Configuration class C { @Bean H h() {} }").unwrap(), ["chat", "c", "h"]);
    }

    #[test]
    fn reports_missing_attributes_and_unknown_handlers() {
        let is_bean = |n: &str| n == "echoHandler";
        let found: Vec<_> = check(XML, Some(&is_bean)).into_iter().map(|f| (f.code, XML[f.start..f.end].to_owned(), f.severity)).collect();
        assert_eq!(
            found,
            [
                ("missing-attribute", "websocket:stomp-endpoint".to_owned(), Severity::Warning),
                ("missing-attribute", "websocket:mapping".to_owned(), Severity::Warning),
                ("unresolved-reference", "ghost".to_owned(), Severity::Info),
            ]
        );
        assert_eq!(check(XML, None).len(), 2);
        assert!(check("<beans><mapping/></beans>", None).is_empty());
    }
}
