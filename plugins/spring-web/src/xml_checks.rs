//! Attribute checks of the `<mvc:...>` namespace of Spring XML configuration,
//! from a hard-coded table since no schema is at hand.

use spring_text::finding::Finding;
use spring_text::xml::{self, Element};
use spring_text::xml_check::{Rule, check as run};

const NAMESPACE: &str = "http://www.springframework.org/schema/mvc";

const BOOLEAN: &[&str] = &["true", "false"];

const RULES: &[Rule] = &[
    Rule { required: &["path"], ..Rule::new("view-controller") },
    Rule {
        required: &["path", "redirect-url"],
        enums: &[("context-relative", BOOLEAN), ("keep-query-params", BOOLEAN)],
        ..Rule::new("redirect-view-controller")
    },
    Rule { enums: &[("ignore-default-model-on-redirect", BOOLEAN)], ..Rule::new("annotation-driven") },
    Rule { enums: &[("register-defaults", BOOLEAN)], ..Rule::new("message-converters") },
    Rule { required: &["resource-cache"], enums: &[("resource-cache", BOOLEAN)], ..Rule::new("resource-chain") },
    Rule { required: &["path", "status-code"], ..Rule::new("status-controller") },
    Rule { required: &["mapping", "location"], ..Rule::new("resources") },
    Rule { required: &["path"], ..Rule::new("mapping") },
    Rule { required: &["path"], ..Rule::new("exclude-mapping") },
];

pub fn check(text: &str) -> Vec<Finding> {
    let all = xml::elements(text);
    let in_mvc: Vec<Element> = (0..all.len()).filter(|&i| xml::namespace(&all, i) == Some(NAMESPACE)).map(|i| all[i].clone()).collect();
    run(&in_mvc, RULES, |_| true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<beans xmlns="http://www.springframework.org/schema/beans" xmlns:mvc="http://www.springframework.org/schema/mvc">
  <mvc:view-controller view-name="home"/>
  <mvc:resources mapping="/static/**" location="/static/"/>
  <mvc:resources mapping="/x/**"/>
  <mvc:interceptors><mvc:interceptor><mvc:mapping path="/a"/><mvc:exclude-mapping/></mvc:interceptor></mvc:interceptors>
  <mapping/>
  <mvc:annotation-driven ignore-default-model-on-redirect="yes"/>
  <mvc:redirect-view-controller path="/a" redirect-url="/b" keep-query-params="${keep}"/>
  <mvc:resource-chain/>
</beans>"#;

    #[test]
    fn reports_missing_attributes_of_mvc_elements_only() {
        let found: Vec<_> = check(XML).into_iter().map(|f| (f.code, XML[f.start..f.end].to_owned(), f.message)).collect();
        let names: Vec<_> = found.iter().map(|f| f.1.as_str()).collect();
        assert_eq!(names, ["mvc:view-controller", "mvc:resources", "mvc:exclude-mapping", "yes", "mvc:resource-chain"]);
        assert!(found[1].2.contains("\"location\""));
    }
}
