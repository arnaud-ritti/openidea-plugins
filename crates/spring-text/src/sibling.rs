//! Calls from a method to another method of the same class, which bypass the
//! Spring proxy: the scan behind `self-invocation` and its siblings.

use crate::java::{self, Member, TypeDecl};

#[derive(Clone, Copy, Debug)]
pub struct SiblingCall<'a> {
    pub callee: &'a Member,
    pub caller: &'a Member,
    /// Bytes of the called name in the source.
    pub at: usize,
    pub end: usize,
}

/// Calls inside `ty` (a class without a superclass) from a method to a distinct
/// non-private, non-final, non-static method with a unique name for which
/// `is_target` holds, written bare or on `this`. A caller for which `excused`
/// holds is skipped.
pub fn sibling_calls<'a>(
    ty: &'a TypeDecl,
    text: &str,
    is_target: impl Fn(&Member) -> bool,
    excused: impl Fn(&Member, &Member) -> bool,
) -> Vec<SiblingCall<'a>> {
    let mut out = Vec::new();
    if !ty.extends.is_empty() {
        return out;
    }
    for callee in ty.methods().filter(|m| is_target(m)) {
        let plain = !["private", "final", "static"].iter().any(|k| callee.modifiers.iter().any(|m| m == k));
        let unique = ty.methods().filter(|m| m.name == callee.name).count() == 1;
        if !plain || !unique {
            continue;
        }
        for caller in ty.methods().filter(|m| m.name != callee.name && !excused(m, callee)) {
            let Some((from, to)) = caller.body else { continue };
            let Some(body) = text.get(from..to) else { continue };
            for call in java::calls(body).into_iter().filter(|c| c.name == callee.name && !c.is_new && !c.chained) {
                if call.receiver.as_deref().is_some_and(|r| r != "this") {
                    continue;
                }
                let at = from + call.at;
                out.push(SiblingCall { callee, caller, at, end: at + call.name.len() });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"class A {
    @Tx public void save() {}
    @Tx private void hidden() {}
    @Tx public void twice() {}
    @Tx public void twice(int x) {}
    public void run() { save(); this.save(); other.save(); hidden(); twice(); new save(); }
    @Tx public void tx() { save(); }
}"#;

    fn scan(src: &str) -> Vec<(String, String)> {
        let file = java::parse(src);
        let tx = |m: &Member| m.annotation("Tx").is_some();
        let excused = |caller: &Member, _: &Member| caller.annotation("Tx").is_some();
        sibling_calls(&file.types[0], src, tx, excused)
            .into_iter()
            .map(|c| (c.caller.name.clone(), c.callee.name.clone()))
            .collect()
    }

    #[test]
    fn finds_bare_and_this_calls_to_plain_unique_targets() {
        let found = scan(SRC);
        let pair = ("run".to_owned(), "save".to_owned());
        assert_eq!(found, [pair.clone(), pair]);
    }

    #[test]
    fn is_quiet_with_a_superclass() {
        assert!(scan(&SRC.replace("class A", "class A extends B")).is_empty());
    }
}
