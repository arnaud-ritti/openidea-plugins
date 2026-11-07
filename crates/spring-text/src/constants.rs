//! `static final String` constants of the project, resolved by simple type
//! name, so a check can read `@RabbitListener(queues = Queues.ORDERS)`.

use crate::java::{self, JavaFile, MemberKind};
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Constants {
    values: HashMap<String, String>,
}

impl Constants {
    /// Records the string constants of every type of a parsed source (Java `static final`, Kotlin `const val`).
    pub fn add(&mut self, file: &JavaFile, text: &str) {
        for ty in &file.types {
            for field in ty.fields() {
                let modifier = |name: &str| field.modifiers.iter().any(|m| m == name);
                let constant = modifier("const") || (modifier("static") && modifier("final"));
                let Some((from, to)) = field.body.filter(|_| constant && field.kind == MemberKind::Field) else { continue };
                let Some(init) = text.get(from..to) else { continue };
                if let Some(value) = self.resolve(init.trim_start().trim_start_matches('='), Some(&ty.name)) {
                    self.values.insert(format!("{}.{}", ty.name, field.name), value);
                }
            }
        }
    }

    /// The value of `expr`: string literals, `Type.NAME` and `NAME` (of `own`) joined by `+`; none when any part is not a known constant.
    pub fn resolve(&self, expr: &str, own: Option<&str>) -> Option<String> {
        let toks = java::tokens(expr);
        let mut out = String::new();
        for operand in toks.split(|t| t.s == "+") {
            if let [single] = operand {
                if let Some(s) = single.string() {
                    out.push_str(&s.value);
                    continue;
                }
            }
            let dotted = operand.iter().enumerate().all(|(i, t)| if i % 2 == 0 { t.is_ident() } else { t.s == "." });
            if operand.is_empty() || !dotted || operand.len() % 2 == 0 {
                return None;
            }
            let path: String = operand.iter().map(|t| t.s).collect();
            out.push_str(self.get(&path, own)?);
        }
        Some(out)
    }

    fn get(&self, path: &str, own: Option<&str>) -> Option<&String> {
        let qualified = match path.rsplit_once('.') {
            Some((ty, name)) => format!("{}.{name}", ty.rsplit('.').next().unwrap_or(ty)),
            None => format!("{}.{path}", own?),
        };
        self.values.get(&qualified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constants() -> Constants {
        let src = r#"class Queues {
    public static final String PREFIX = "orders";
    public static final String ORDERS = PREFIX + ".in";
    public static final String DYNAMIC = compute();
    static String mutable = "x";
}"#;
        let mut c = Constants::default();
        c.add(&java::parse(src), src);
        c
    }

    #[test]
    fn resolves_literals_and_chained_constants() {
        let c = constants();
        assert_eq!(c.resolve("Queues.ORDERS", None).as_deref(), Some("orders.in"));
        assert_eq!(c.resolve("com.acme.Queues.PREFIX + \"-x\"", None).as_deref(), Some("orders-x"));
        assert_eq!(c.resolve("ORDERS", Some("Queues")).as_deref(), Some("orders.in"));
        assert_eq!(c.resolve("\"plain\"", None).as_deref(), Some("plain"));
    }

    #[test]
    fn is_quiet_for_anything_else() {
        let c = constants();
        for expr in ["Queues.DYNAMIC", "Queues.mutable", "Other.X", "ORDERS", "Queues.PREFIX +", "", "foo()"] {
            assert_eq!(c.resolve(expr, None), None, "{expr}");
        }
    }

    #[test]
    fn reads_kotlin_const_val() {
        let src = "object Q {\n    const val A = \"a\"\n}";
        let mut c = Constants::default();
        c.add(&crate::kotlin::parse(src), src);
        assert_eq!(c.resolve("Q.A", None).as_deref(), Some("a"));
    }
}
