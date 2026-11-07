//! A table-driven check of XML elements: required attributes, enumerated
//! values and attributes that name a bean, for namespaces without a schema at
//! hand. A table lists only elements it knows completely.

use crate::finding::Finding;
use crate::xml::Element;

#[derive(Clone, Copy, Debug)]
pub struct Rule {
    /// Local element name.
    pub element: &'static str,
    /// Attributes that must be present; `a|b` accepts either.
    pub required: &'static [&'static str],
    /// Attribute and the values it accepts.
    pub enums: &'static [(&'static str, &'static [&'static str])],
    /// Attributes holding a bean name.
    pub refs: &'static [&'static str],
}

impl Rule {
    pub const fn new(element: &'static str) -> Self {
        Self { element, required: &[], enums: &[], refs: &[] }
    }
}

/// Findings (`missing-attribute`, `invalid-attribute-value`, `unresolved-reference`) of the elements
/// matching a rule by local name. A value with `${` or `#{` is skipped; `is_bean` tells whether a ref resolves.
pub fn check(elements: &[Element], rules: &[Rule], is_bean: impl Fn(&str) -> bool) -> Vec<Finding> {
    let mut out = Vec::new();
    for element in elements {
        for rule in rules.iter().filter(|r| r.element == element.local()) {
            for required in rule.required {
                if required.split('|').any(|name| element.attr(name).is_some()) {
                    continue;
                }
                out.push(Finding::new(
                    element.at,
                    element.at + element.name.len(),
                    "missing-attribute",
                    format!("<{}> needs the attribute \"{}\".", element.name, required.replace('|', "\" or \"")),
                ));
            }
            for (name, allowed) in rule.enums {
                let Some(attr) = element.attr(name) else { continue };
                if is_placeholder(&attr.value) || allowed.contains(&attr.value.as_str()) {
                    continue;
                }
                out.push(Finding::new(
                    attr.value_at,
                    attr.value_end,
                    "invalid-attribute-value",
                    format!("\"{}\" is not a value of {name}; use {}.", attr.value, allowed.join(", ")),
                ));
            }
            for name in rule.refs {
                let Some(attr) = element.attr(name) else { continue };
                if is_placeholder(&attr.value) || attr.value.is_empty() || is_bean(&attr.value) {
                    continue;
                }
                out.push(Finding::new(
                    attr.value_at,
                    attr.value_end,
                    "unresolved-reference",
                    format!("No bean named \"{}\" is defined in the project.", attr.value),
                ));
            }
        }
    }
    out
}

fn is_placeholder(value: &str) -> bool {
    value.contains("${") || value.contains("#{")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::elements;

    const RULES: &[Rule] = &[Rule {
        element: "listener",
        required: &["ref", "destination|queue"],
        enums: &[("ack", &["auto", "manual"])],
        refs: &["ref"],
    }];

    fn run(xml: &str) -> Vec<(&'static str, String)> {
        let found = check(&elements(xml), RULES, |b| b == "known");
        found.into_iter().map(|f| (f.code, xml[f.start..f.end].to_owned())).collect()
    }

    #[test]
    fn reports_missing_enum_and_ref_problems() {
        let xml = r#"<r><x:listener ack="sometimes" ref="nope"/></r>"#;
        assert_eq!(
            run(xml),
            [
                ("missing-attribute", "x:listener".to_owned()),
                ("invalid-attribute-value", "sometimes".to_owned()),
                ("unresolved-reference", "nope".to_owned()),
            ]
        );
    }

    #[test]
    fn is_quiet_for_valid_placeholder_and_unlisted() {
        assert!(run(r#"<listener ref="known" queue="q" ack="auto"/>"#).is_empty());
        assert!(run(r##"<listener ref="${r}" destination="d" ack="#{a}"/>"##).is_empty());
        assert!(run(r#"<other/>"#).is_empty());
    }
}
