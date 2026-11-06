use crate::NAMESPACE;
use openidea_plugin::Diagnostic;
use spring_text::xml::{self, Element};
use spring_text::xml_check::{Rule, check};

const BOOLEAN: &[&str] = &["true", "false"];

/// Written by hand from the Spring Batch XSD: only elements listed completely.
const RULES: &[Rule] = &[
    Rule { element: "job", required: &["id"], enums: &[("restartable", BOOLEAN), ("abstract", BOOLEAN)], refs: &[] },
    Rule { element: "step", required: &["id"], enums: &[("abstract", BOOLEAN)], refs: &[] },
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
