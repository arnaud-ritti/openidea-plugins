use crate::NAMESPACE;
use openidea_plugin::Diagnostic;
use spring_text::xml::{self, Element};
use spring_text::xml_check::{Rule, check};

const BOOLEAN: &[&str] = &["true", "false"];
const ISOLATION: &[&str] = &["DEFAULT", "READ_UNCOMMITTED", "READ_COMMITTED", "REPEATABLE_READ", "SERIALIZABLE"];
const PROPAGATION: &[&str] = &["REQUIRED", "REQUIRES_NEW", "SUPPORTS", "NOT_SUPPORTED", "MANDATORY", "NEVER", "NESTED"];

/// Written by hand from the Spring Batch XSD: only elements listed completely.
const RULES: &[Rule] = &[
    Rule { element: "job", required: &["id"], enums: &[("restartable", BOOLEAN), ("abstract", BOOLEAN)], refs: &[] },
    Rule { element: "step", required: &["id"], enums: &[("abstract", BOOLEAN), ("allow-start-if-complete", BOOLEAN)], refs: &[] },
    Rule { element: "transaction-attributes", required: &[], enums: &[("isolation", ISOLATION), ("propagation", PROPAGATION)], refs: &[] },
    Rule { element: "include", required: &["class"], enums: &[], refs: &[] },
    Rule { element: "exclude", required: &["class"], enums: &[], refs: &[] },
    Rule { element: "stream", required: &["ref"], enums: &[], refs: &[] },
    Rule { element: "split", required: &["id"], enums: &[], refs: &[] },
    Rule { element: "decision", required: &["id", "decider"], enums: &[], refs: &[] },
    Rule { element: "next", required: &["on", "to"], enums: &[], refs: &[] },
    Rule { element: "end", required: &["on"], enums: &[], refs: &[] },
    Rule { element: "fail", required: &["on"], enums: &[], refs: &[] },
    Rule { element: "stop", required: &["on", "restart"], enums: &[], refs: &[] },
    Rule { element: "tasklet", required: &[], enums: &[("allow-start-if-complete", BOOLEAN)], refs: &[] },
    Rule {
        element: "chunk",
        required: &[],
        enums: &[("reader-transactional-queue", BOOLEAN), ("processor-transactional", BOOLEAN)],
        refs: &[],
    },
];

/// Missing required attributes and wrong boolean values of the batch namespace.
pub fn problems(text: &str) -> Vec<Diagnostic> {
    let all = xml::elements(text);
    let batch: Vec<Element> =
        all.iter().enumerate().filter(|(i, _)| xml::namespace(&all, *i) == Some(NAMESPACE)).map(|(_, e)| e.clone()).collect();
    check(&batch, RULES, |_| true).iter().map(|f| f.diagnostic(text)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_transaction_attributes_and_exception_classes() {
        let xml = r#"<job id="j" xmlns="http://www.springframework.org/schema/batch"><step id="s"><tasklet><transaction-attributes isolation="SERIAL" propagation="NESTED"/><chunk commit-interval="1"><skippable-exception-classes><include/><exclude class="E"/></skippable-exception-classes><streams><stream/></streams></chunk></tasklet></step></job>"#;
        let found: Vec<_> = problems(xml).iter().map(|d| (d.code.clone().unwrap(), xml[..].lines().next().map(|l| l[d.range.start.character as usize..d.range.end.character as usize].to_owned()).unwrap())).collect();
        assert_eq!(
            found,
            [
                ("invalid-attribute-value".to_owned(), "SERIAL".to_owned()),
                ("missing-attribute".into(), "include".into()),
                ("missing-attribute".into(), "stream".into()),
            ]
        );
    }
}
