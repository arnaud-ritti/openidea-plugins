//! Problems in entities and repositories, each with an optional quick fix.

use openidea_plugin::text::position;
use openidea_plugin::{Diagnostic, Range, Severity};

use crate::model::{Annotation, Edit, annotations, has_word, import_edit, kotlin_supertype, mask, words};
use crate::queries::{Props, Repository, boxed, problem};

#[derive(Debug)]
pub struct Finding {
    pub start: usize,
    pub end: usize,
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub fix: Option<(&'static str, Vec<Edit>)>,
}

impl Finding {
    fn new(start: usize, end: usize, severity: Severity, code: &'static str, message: impl Into<String>) -> Self {
        Self { start, end, severity, code, message: message.into(), fix: None }
    }

    fn fix(mut self, title: &'static str, edits: Vec<Edit>) -> Self {
        self.fix = Some((title, edits));
        self
    }

    pub fn diagnostic(&self, text: &str) -> Diagnostic {
        let range = Range::new(position(text, self.start), position(text, self.end));
        Diagnostic::new(range, self.severity, self.message.clone()).code(self.code)
    }
}

fn fetch_type(text: &str) -> &'static str {
    if text.contains("javax.persistence") {
        return "javax.persistence.FetchType";
    }

    "jakarta.persistence.FetchType"
}

/// Lazy fetching for an association: explicit `EAGER` on any of them, the eager default of a to-one.
fn fetch_finding(text: &str, masked: &str, a: &Annotation, to_one: bool, kotlin: bool) -> Option<Finding> {
    let args = a.args.map(|(start, end)| (start, &masked[start..end]));
    let lazy = "FetchType.LAZY";
    let mut edits: Vec<Edit> = import_edit(text, &[fetch_type(text)], kotlin).into_iter().collect();

    if let Some(eager) = args.and_then(|(start, inner)| words(inner, "EAGER").next().map(|i| start + i)) {
        let from = if masked[..eager].ends_with("FetchType.") { eager - "FetchType.".len() } else { eager };
        edits.push(Edit { start: from, end: eager + "EAGER".len(), text: lazy.into() });

        let finding = Finding::new(
            a.start,
            a.end(),
            Severity::Warning,
            "jpa-eager-fetch",
            "Eager fetching loads this association with every query on the entity. Use LAZY and fetch it explicitly where you need it.",
        );

        return Some(finding.fix("Fetch lazily", edits));
    }

    if !to_one {
        return None;
    }

    if args.is_some_and(|(_, inner)| has_word(inner, "LAZY") || has_word(inner, "mappedBy")) {
        return None;
    }

    edits.push(match a.args {
        None => Edit::insert(a.name_end, &format!("(fetch = {lazy})")),
        Some((start, end)) if masked[start..end].trim().is_empty() => Edit::insert(start, &format!("fetch = {lazy}")),
        Some((start, _)) => Edit::insert(start, &format!("fetch = {lazy}, ")),
    });

    let finding = Finding::new(
        a.start,
        a.end(),
        Severity::Info,
        "jpa-eager-to-one",
        "To-one associations are fetched eagerly by default. Set fetch = FetchType.LAZY.",
    );

    Some(finding.fix("Fetch lazily", edits))
}

fn lombok_data_fix(text: &str, a: &Annotation, kotlin: bool) -> Vec<Edit> {
    let line_start = text[..a.start].rfind('\n').map_or(0, |p| p + 1);
    let indent = &text[line_start..a.start];
    let separator = if indent.trim().is_empty() { format!("\n{indent}") } else { " ".into() };

    let mut edits: Vec<Edit> = import_edit(text, &["lombok.Getter", "lombok.Setter"], kotlin).into_iter().collect();
    edits.push(Edit { start: a.start, end: a.end(), text: format!("@Getter{separator}@Setter") });

    edits
}

/// Problems of the entity declared in `text`.
pub fn inspect_entity(text: &str, kotlin: bool) -> Vec<Finding> {
    let masked = mask(text);
    let annotations = annotations(&masked);
    let Some(entity) = annotations.iter().find(|a| a.name == "Entity") else { return Vec::new() };
    let mut findings = Vec::new();

    if let Some(class_at) = words(&masked[entity.end()..], "class").next().map(|i| i + entity.end()) {
        let modifiers = &masked[entity.end()..class_at];

        if let Some(at) = words(modifiers, "final").next().filter(|_| !kotlin).map(|i| i + entity.end()) {
            let end = at + "final".len();
            let removed = end + masked[end..].bytes().take_while(u8::is_ascii_whitespace).count();
            findings.push(
                Finding::new(
                    at,
                    end,
                    Severity::Warning,
                    "jpa-final-entity",
                    "A final entity can't be proxied, so its lazy associations are loaded eagerly.",
                )
                .fix("Remove final", vec![Edit { start: at, end: removed, text: String::new() }]),
            );
        }

        if let Some(at) = words(modifiers, "data").next().filter(|_| kotlin).map(|i| i + entity.end()) {
            findings.push(Finding::new(
                at,
                at + "data".len(),
                Severity::Warning,
                "jpa-data-class",
                "A data class entity gets equals, hashCode and toString over every property, lazy associations included.",
            ));
        }

        let header_end = masked[class_at..].find('{').map_or(masked.len(), |p| p + class_at);
        let header = &masked[class_at..header_end];
        let inherits = if kotlin { kotlin_supertype(header).is_some() } else { has_word(header, "extends") };

        if !inherits && !annotations.iter().any(|a| a.name == "Id" || a.name == "EmbeddedId") {
            findings.push(Finding::new(
                entity.start,
                entity.end(),
                Severity::Error,
                "jpa-missing-id",
                "Entity has no @Id or @EmbeddedId attribute.",
            ));
        }
    }

    for a in &annotations {
        let finding = match a.name {
            "Data" => Some(
                Finding::new(
                    a.start,
                    a.end(),
                    Severity::Warning,
                    "jpa-lombok-data",
                    "@Data on an entity generates equals, hashCode and toString over every field, lazy associations included.",
                )
                .fix("Replace with @Getter and @Setter", lombok_data_fix(text, a, kotlin)),
            ),
            "EqualsAndHashCode" => Some(Finding::new(
                a.start,
                a.end(),
                Severity::Warning,
                "jpa-lombok-equals",
                "The generated equals and hashCode compare mutable fields and a generated id, so the entity's hash code changes once it is persisted.",
            )),
            "ManyToOne" | "OneToOne" => fetch_finding(text, &masked, a, true, kotlin),
            "OneToMany" | "ManyToMany" | "ElementCollection" => fetch_finding(text, &masked, a, false, kotlin),
            _ => None,
        };

        findings.extend(finding);
    }

    findings
}

/// Derived queries naming unknown properties, and an id type that isn't the entity's.
pub fn inspect_repository(repo: &Repository, entity_id: Option<&str>, props: Props) -> Vec<Finding> {
    let mut findings = Vec::new();

    if props(&repo.entity).is_none() {
        return findings;
    }

    let same = |a: &str, b: &str| {
        let java = |t: &str| boxed(match t {
            "Int" => "Integer",
            "Char" => "Character",
            other => other,
        });
        java(a) == java(b)
    };

    if let Some(id) = entity_id.filter(|id| !same(id, &repo.id_type)) {
        findings.push(Finding::new(
            repo.id_start,
            repo.id_end,
            Severity::Error,
            "jpa-repository-id",
            format!("The id of {} is {id}, not {}.", repo.entity, repo.id_type),
        ));
    }

    for method in repo.methods.iter().filter(|m| !m.declared_query) {
        if let Some(message) = problem(&method.name, &repo.entity, props) {
            findings.push(Finding::new(method.start, method.end, Severity::Error, "jpa-derived-query", message));
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queries::{Prop, parse_repository};

    const ENTITY: &str = "package shop;

import jakarta.persistence.Entity;
import jakarta.persistence.Id;
import jakarta.persistence.ManyToOne;
import lombok.Data;

// @OneToMany(fetch = FetchType.EAGER) in a comment is ignored
@Data
@Entity
public final class Order {
    @Id
    private Long id;

    @ManyToOne
    private Customer customer;

    @ManyToOne(fetch = FetchType.LAZY)
    private Customer lazy;

    @OneToOne(mappedBy = \"order\")
    private Invoice invoice;
}
";

    fn codes(text: &str, kotlin: bool) -> Vec<&'static str> {
        inspect_entity(text, kotlin).iter().map(|f| f.code).collect()
    }

    fn apply(text: &str, mut edits: Vec<Edit>) -> String {
        let mut out = text.to_string();
        edits.sort_by_key(|e| std::cmp::Reverse(e.start));
        for e in edits {
            out.replace_range(e.start..e.end, &e.text);
        }
        out
    }

    fn fixed(text: &str, code: &str, kotlin: bool) -> String {
        let finding = inspect_entity(text, kotlin).into_iter().find(|f| f.code == code).unwrap();
        apply(text, finding.fix.unwrap().1)
    }

    #[test]
    fn reports_entity_problems_and_ignores_comments_and_inverse_sides() {
        assert_eq!(codes(ENTITY, false), ["jpa-final-entity", "jpa-lombok-data", "jpa-eager-to-one"]);
        assert!(codes(&ENTITY.replace("@Entity", "@Embeddable"), false).is_empty());
    }

    #[test]
    fn missing_id_unless_inherited() {
        assert_eq!(codes("@Entity\nclass A {\n  String name;\n}\n", false), ["jpa-missing-id"]);
        assert!(codes("@Entity\nclass A extends Base {}\n", false).is_empty());
        assert!(codes("@Entity\nclass A(@EmbeddedId val id: Key)\n", true).is_empty());
        assert!(codes("@Entity\nclass A(name: String) : Base()\n", true).is_empty());
        assert_eq!(codes("@Entity\ndata class A(val name: String)\n", true), ["jpa-data-class", "jpa-missing-id"]);
    }

    #[test]
    fn fixes_fetching_with_the_import() {
        let lazy = fixed(ENTITY, "jpa-eager-to-one", false);
        assert!(lazy.contains("@ManyToOne(fetch = FetchType.LAZY)\n    private Customer customer;"));
        assert!(lazy.contains("import lombok.Data;\nimport jakarta.persistence.FetchType;\n"));

        let eager = "import javax.persistence.*\n\n@Entity\nclass A {\n  @Id var id: Long? = null\n  @OneToMany(mappedBy = \"a\", fetch = FetchType.EAGER) val b: List<B> = listOf()\n}\n";
        let fixed_eager = fixed(eager, "jpa-eager-fetch", true);
        assert!(fixed_eager.contains("@OneToMany(mappedBy = \"a\", fetch = FetchType.LAZY)"));
        assert!(fixed_eager.starts_with("import javax.persistence.*\n\n@Entity"), "no import for a wildcard");

        let with_args = "@Entity class A { @Id Long id; @ManyToOne(optional = false) B b; }";
        assert!(fixed(with_args, "jpa-eager-to-one", false).contains("@ManyToOne(fetch = FetchType.LAZY, optional = false)"));
    }

    #[test]
    fn replaces_data_and_drops_final() {
        let getters = fixed(ENTITY, "jpa-lombok-data", false);
        assert!(getters.contains("\n@Getter\n@Setter\n@Entity\n"));
        assert!(getters.contains("import lombok.Data;\nimport lombok.Getter;\nimport lombok.Setter;\n"));
        assert!(fixed(ENTITY, "jpa-final-entity", false).contains("\npublic class Order {"));
    }

    fn props(class: &str) -> Option<Vec<Prop>> {
        (class == "Order").then(|| vec![Prop { name: "number".into(), ty: "String".into(), fqcn: None, target: None }])
    }

    #[test]
    fn checks_repository_methods_and_id_type() {
        let text = "interface R extends JpaRepository<Order, Integer> {\n  List<Order> findByNumber(String n);\n  List<Order> findByCode(String c);\n  @Query(\"x\") List<Order> findByWhatever();\n}";
        let repo = parse_repository(text, false).unwrap();
        let found = inspect_repository(&repo, Some("long"), &props);
        let messages: Vec<&str> = found.iter().map(|f| f.message.as_str()).collect();
        assert_eq!(messages, ["The id of Order is long, not Integer.", "Order has no property 'code' (at 'Code')."]);
        assert_eq!(&text[found[1].start..found[1].end], "findByCode");

        assert!(inspect_repository(&repo, Some("Integer"), &props).len() == 1);
        let kotlin = parse_repository("interface R : JpaRepository<Order, Int>", true).unwrap();
        assert!(inspect_repository(&kotlin, Some("Integer"), &props).is_empty());
        let unknown = parse_repository("interface R : JpaRepository<Other, Int> { fun findByX(): List<Other> }", true).unwrap();
        assert!(inspect_repository(&unknown, None, &props).is_empty());
    }
}
