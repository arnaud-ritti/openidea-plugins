use std::ops::Range;

use crate::lex;

pub const UNCLOSED_TAG: &str = "unclosed-tag";
pub const UNEXPECTED_END_TAG: &str = "unexpected-end-tag";
pub const UNTERMINATED: &str = "unterminated-delimiter";

const PAIRED: &[&str] = &["apply", "autoescape", "block", "cache", "embed", "for", "if", "macro", "sandbox", "set", "verbatim", "with"];

pub struct Finding {
    pub code: &'static str,
    pub range: Range<usize>,
    pub message: String,
}

pub fn check(text: &str) -> Vec<Finding> {
    let lexed = lex(text);
    let mut findings = Vec::new();
    let mut open: Vec<(&str, &Range<usize>)> = Vec::new();

    for tag in &lexed.tags {
        if let Some(name) = tag.name.strip_prefix("end") {
            if !PAIRED.contains(&name) {
                continue;
            }

            let Some(at) = open.iter().rposition(|(n, _)| *n == name) else {
                findings.push(Finding {
                    code: UNEXPECTED_END_TAG,
                    range: tag.range.clone(),
                    message: format!("{{% end{name} %}} has no matching {{% {name} %}}."),
                });
                continue;
            };

            for (name, range) in open.split_off(at + 1) {
                findings.push(unclosed(name, range));
            }
            open.pop();
            continue;
        }

        if tag.paired && PAIRED.contains(&tag.name.as_str()) {
            open.push((tag.name.as_str(), &tag.range));
        }
    }

    findings.extend(open.into_iter().map(|(name, range)| unclosed(name, range)));
    findings.extend(lexed.unterminated.iter().map(|range| Finding {
        code: UNTERMINATED,
        range: range.clone(),
        message: format!("\"{}\" is never closed.", &text[range.clone()]),
    }));
    findings
}

fn unclosed(name: &str, range: &Range<usize>) -> Finding {
    Finding {
        code: UNCLOSED_TAG,
        range: range.clone(),
        message: format!("Tag \"{name}\" is never closed: missing {{% end{name} %}}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(text: &str) -> Vec<(&'static str, String)> {
        check(text).into_iter().map(|f| (f.code, text[f.range].to_owned())).collect()
    }

    #[test]
    fn balanced_template_is_clean() {
        let text = "{% extends 'a' %}{% block x %}{% if a %}{% else %}{% endif %}{% endblock %}{% block t 'T' %}{% set a = 1 %}{% verbatim %}{% if %}{% endverbatim %}";
        assert!(codes(text).is_empty());
    }

    #[test]
    fn reports_unclosed_and_unexpected_end_tags() {
        assert_eq!(codes("{% for a in b %}{% if a %}{% endfor %}"), [("unclosed-tag", "{% if a %}".into())]);
        assert_eq!(codes("{% endif %}"), [("unexpected-end-tag", "{% endif %}".into())]);
        assert_eq!(codes("{% set a %}x"), [("unclosed-tag", "{% set a %}".into())]);
    }

    #[test]
    fn reports_unterminated_delimiters() {
        assert_eq!(codes("<p>{{ name </p>"), [("unterminated-delimiter", "{{".into())]);
        assert_eq!(codes("{# note"), [("unterminated-delimiter", "{#".into())]);
        assert!(codes("{{ '}}' }}").is_empty());
    }
}
