//! Lexical scan of AspectJ pointcut expressions for designators that Spring
//! AOP's proxy-based weaving does not support.

use openidea_plugin::Severity;
use spring_text::finding::Finding;

pub const SPRING_UNSUPPORTED: [&str; 14] = [
    "call",
    "get",
    "set",
    "preinitialization",
    "staticinitialization",
    "initialization",
    "handler",
    "adviceexecution",
    "withincode",
    "cflow",
    "cflowbelow",
    "if",
    "@this",
    "@withincode",
];

/// Designators at the top level or inside grouping parentheses. The parentheses
/// of a designator are skipped whole, so `execution(* get(..))` never matches.
pub fn unsupported_designators(expression: &str) -> Vec<(usize, usize, &'static str)> {
    let bytes = expression.as_bytes();
    let word = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$' | b'.' | b'*' | b'@') || b >= 0x80;
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !word(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && word(bytes[i]) {
            i += 1;
        }
        let end = i;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if bytes.get(i) != Some(&b'(') {
            continue;
        }
        if let Some(name) = SPRING_UNSUPPORTED.iter().find(|n| **n == &expression[start..end]) {
            out.push((start, end, *name));
        }
        let mut depth = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            i += 1;
            if depth == 0 {
                break;
            }
        }
    }
    out
}

/// Findings for `expression`, whose first byte sits at `base` in the file.
pub fn findings(expression: &str, base: usize) -> Vec<Finding> {
    unsupported_designators(expression)
        .into_iter()
        .map(|(start, end, name)| {
            let message = format!(
                "Spring AOP only intercepts method execution on beans, so the `{name}` pointcut designator is not supported here; it needs the AspectJ compiler or load-time weaving."
            );
            Finding::new(base + start, base + end, "unsupported-pointcut-designator", message).with_severity(Severity::Error)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(expression: &str) -> Vec<&'static str> {
        unsupported_designators(expression).into_iter().map(|(_, _, n)| n).collect()
    }

    #[test]
    fn finds_call_cflow_and_at_this() {
        let expression = "call(* *(..)) && cflow(execution(* a(..))) && @this(Foo)";
        assert_eq!(names(expression), ["call", "cflow", "@this"]);
        let (start, end, _) = unsupported_designators(expression)[2];
        assert_eq!(&expression[start..end], "@this");
    }

    #[test]
    fn ignores_method_named_like_a_designator() {
        assert!(names("execution(* get(..))").is_empty());
        assert!(names("execution(* *.call(..))").is_empty());
    }

    #[test]
    fn descends_into_grouping_and_negation() {
        assert_eq!(names("!(call(* *(..)) || within(a..*))"), ["call"]);
    }

    #[test]
    fn ignores_named_pointcut_references() {
        assert!(names("inApp() && com.acme.Pointcuts.get() && within(a..*)").is_empty());
    }
}
