//! Parameter and return types of derived query methods, judged only when both
//! sides are well-known simple types that cannot convert into one another.

use spring_text::java::{Member, TypeRef};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Number,
    Text,
    Bool,
    Temporal,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Number => "a number",
            Kind::Text => "text",
            Kind::Bool => "a boolean",
            Kind::Temporal => "a date or time",
        }
    }
}

pub fn kind_of(name: &str) -> Option<Kind> {
    let name = name.trim_end_matches('?');
    Some(match name {
        "int" | "Integer" | "long" | "Long" | "short" | "Short" | "byte" | "Byte" | "double" | "Double" | "float" | "Float"
        | "BigDecimal" | "BigInteger" | "Int" | "Number" => Kind::Number,
        "String" => Kind::Text,
        "boolean" | "Boolean" => Kind::Bool,
        "Instant" | "LocalDate" | "LocalDateTime" | "LocalTime" | "ZonedDateTime" | "OffsetDateTime" | "OffsetTime" | "Date"
        | "Timestamp" | "Year" | "YearMonth" => Kind::Temporal,
        _ => return None,
    })
}

/// A message when a parameter of type `param` can never be compared with a property of type `property`.
pub fn mismatch(property: &str, param: &TypeRef) -> Option<String> {
    if param.array {
        return None;
    }
    let (expected, given) = (kind_of(property)?, kind_of(&param.name)?);
    (expected != given).then(|| {
        format!("`{property}` holds {}, but this parameter is `{}`, {}.", expected.label(), param.name, given.label())
    })
}

const COLLECTIONS: [&str; 6] = ["List", "Set", "Collection", "Iterable", "Page", "Slice"];
const STREAMS: [&str; 2] = ["Stream", "Streamable"];

/// A message when the verb of a derived query method cannot produce its return type.
pub fn return_problem(method: &Member) -> Option<String> {
    let ty = method.ty.as_ref()?;
    let name = method.name.as_str();
    let many = COLLECTIONS.contains(&ty.name.as_str()) || STREAMS.contains(&ty.name.as_str());
    let verb = ["count", "exists", "delete", "remove"].iter().find(|v| name.starts_with(**v))?;
    let message = match *verb {
        "count" if many || kind_of(&ty.name) == Some(Kind::Text) || kind_of(&ty.name) == Some(Kind::Bool) => {
            "A `count` query returns a number (`long`)."
        }
        "exists" if many || kind_of(&ty.name) == Some(Kind::Text) || kind_of(&ty.name) == Some(Kind::Number) => {
            "An `exists` query returns a `boolean`."
        }
        "delete" | "remove" if ty.name == "Page" || ty.name == "Slice" => "A `delete` query cannot return a page.",
        _ => return None,
    };
    Some(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use spring_text::java::parse;

    fn param(src: &str) -> TypeRef {
        parse(&format!("interface R {{ void f({src} p); }}")).types[0].methods().next().unwrap().params[0].ty.clone()
    }

    #[test]
    fn only_clearly_different_kinds_mismatch() {
        assert!(mismatch("Integer", &param("String")).is_some());
        assert!(mismatch("String", &param("long")).is_some());
        assert!(mismatch("Instant", &param("Boolean")).is_some());
        assert!(mismatch("Integer", &param("Long")).is_none());
        assert!(mismatch("Role", &param("String")).is_none());
        assert!(mismatch("String", &param("Role")).is_none());
        assert!(mismatch("Integer", &param("String[]")).is_none());
    }

    fn method(src: &str) -> Member {
        parse(&format!("interface R {{ {src}; }}")).types[0].methods().next().unwrap().clone()
    }

    #[test]
    fn verbs_constrain_the_return_type() {
        assert!(return_problem(&method("List<User> countByName(String n)")).is_some());
        assert!(return_problem(&method("long countByName(String n)")).is_none());
        assert!(return_problem(&method("String existsByName(String n)")).is_some());
        assert!(return_problem(&method("boolean existsByName(String n)")).is_none());
        assert!(return_problem(&method("Page<User> deleteByName(String n)")).is_some());
        assert!(return_problem(&method("List<User> deleteByName(String n)")).is_none());
        assert!(return_problem(&method("Mono<Long> countByName(String n)")).is_none());
    }
}
