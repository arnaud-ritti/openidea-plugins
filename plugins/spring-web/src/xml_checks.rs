//! Attribute checks of the `<mvc:...>` namespace of Spring XML configuration,
//! from a hard-coded table since no schema is at hand.

use spring_text::finding::Finding;
use spring_text::xml::{self, Element};
use spring_text::xml_check::{Rule, check as run};

const NAMESPACE: &str = "http://www.springframework.org/schema/mvc";

const RULES: &[Rule] = &[
    Rule { required: &["path"], ..Rule::new("view-controller") },
    Rule { required: &["path", "redirect-url"], ..Rule::new("redirect-view-controller") },
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
</beans>"#;

    #[test]
    fn reports_missing_attributes_of_mvc_elements_only() {
        let found: Vec<_> = check(XML).into_iter().map(|f| (f.code, XML[f.start..f.end].to_owned(), f.message)).collect();
        let names: Vec<_> = found.iter().map(|f| f.1.as_str()).collect();
        assert_eq!(names, ["mvc:view-controller", "mvc:resources", "mvc:exclude-mapping"]);
        assert!(found[1].2.contains("\"location\""));
    }
}
