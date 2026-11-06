//! Source for the New Entity and New Attribute wizards.

use crate::generate::callback_import;
use crate::model::{Class, Edit, Relation, capitalize, import_edit};

pub struct EntitySpec {
    pub name: String,
    pub table: String,
    pub package: Option<String>,
    pub kotlin: bool,
    /// `none`, `IDENTITY`, `SEQUENCE`, `AUTO` or `UUID`.
    pub id: String,
    pub auditing: bool,
    pub version: bool,
    pub equals: bool,
}

pub struct AttributeSpec {
    pub name: String,
    /// The Java name of the type, or the target entity of an association.
    pub ty: String,
    pub relation: Relation,
    pub enumerated: bool,
    pub not_null: bool,
    pub unique: bool,
    pub cascade: bool,
    pub fetch: bool,
}

struct Field {
    annotations: Vec<String>,
    ty: String,
    name: String,
    init: Option<String>,
}

const PERSISTENCE: &str = "jakarta.persistence";

fn kotlin_type(ty: &str) -> &str {
    match ty {
        "Integer" => "Int",
        other => other,
    }
}

fn type_import(ty: &str) -> Option<&'static str> {
    match ty {
        "BigDecimal" => Some("java.math.BigDecimal"),
        "LocalDate" => Some("java.time.LocalDate"),
        "LocalDateTime" => Some("java.time.LocalDateTime"),
        "Instant" => Some("java.time.Instant"),
        "UUID" => Some("java.util.UUID"),
        _ => None,
    }
}

fn render(field: &Field, kotlin: bool, accessors: bool) -> String {
    let mut out: String = field.annotations.iter().map(|a| format!("    {a}\n")).collect();
    let Field { ty, name, init, .. } = field;

    if kotlin {
        match init {
            Some(init) => out.push_str(&format!("    var {name}: {ty} = {init}")),
            None => out.push_str(&format!("    var {name}: {ty}? = null")),
        }
        return out;
    }

    match init {
        Some(init) => out.push_str(&format!("    private {ty} {name} = {init};")),
        None => out.push_str(&format!("    private {ty} {name};")),
    }

    if accessors {
        let cap = capitalize(name);
        out.push_str(&format!("\n\n    public {ty} get{cap}() {{\n        return {name};\n    }}"));
        out.push_str(&format!("\n\n    public void set{cap}({ty} {name}) {{\n        this.{name} = {name};\n    }}"));
    }

    out
}

pub fn entity_source(spec: &EntitySpec) -> String {
    let kotlin = spec.kotlin;
    let name = &spec.name;
    let persistence = |what: &str| format!("{PERSISTENCE}.{what}");
    let mut imports = vec![persistence("Entity"), persistence("Table"), persistence("Id")];
    let mut class_annotations = vec!["@Entity".to_owned(), format!("@Table(name = \"{}\")", spec.table)];
    let mut fields = Vec::new();

    let id_type = if spec.id == "UUID" { "UUID" } else { "Long" };
    let mut id_annotations = vec!["@Id".to_owned()];
    if spec.id != "none" {
        imports.extend([persistence("GeneratedValue"), persistence("GenerationType")]);
        id_annotations.push(format!("@GeneratedValue(strategy = GenerationType.{})", spec.id));
    }
    imports.extend(type_import(id_type).map(str::to_owned));
    fields.push(Field { annotations: id_annotations, ty: id_type.into(), name: "id".into(), init: None });

    if spec.version {
        imports.push(persistence("Version"));
        fields.push(Field { annotations: vec!["@Version".into()], ty: "Long".into(), name: "version".into(), init: None });
    }

    if spec.auditing {
        imports.extend([
            persistence("Column"),
            persistence("EntityListeners"),
            "org.springframework.data.annotation.CreatedDate".into(),
            "org.springframework.data.annotation.LastModifiedDate".into(),
            "org.springframework.data.jpa.domain.support.AuditingEntityListener".into(),
            "java.time.Instant".into(),
        ]);
        let listener = if kotlin { "AuditingEntityListener::class" } else { "AuditingEntityListener.class" };
        class_annotations.push(format!("@EntityListeners({listener})"));
        fields.push(Field {
            annotations: vec!["@CreatedDate".into(), "@Column(updatable = false)".into()],
            ty: "Instant".into(),
            name: "createdAt".into(),
            init: None,
        });
        fields.push(Field { annotations: vec!["@LastModifiedDate".into()], ty: "Instant".into(), name: "updatedAt".into(), init: None });
    }

    let mut members: Vec<String> = fields.iter().map(|f| render(f, kotlin, true)).collect();

    if spec.equals {
        members.push(equals_and_hash_code(name, kotlin));
    }

    imports.sort();
    imports.dedup();
    let semicolon = if kotlin { "" } else { ";" };
    let package = spec.package.as_ref().map(|p| format!("package {p}{semicolon}\n\n")).unwrap_or_default();
    let imports: String = imports.iter().map(|i| format!("import {i}{semicolon}\n")).collect();
    let annotations = class_annotations.join("\n");
    let header = if kotlin { format!("class {name}") } else { format!("public class {name}") };

    format!("{package}{imports}\n{annotations}\n{header} {{\n\n{}\n}}\n", members.join("\n\n"))
}

fn equals_and_hash_code(name: &str, kotlin: bool) -> String {
    if kotlin {
        return format!(
            "    override fun equals(other: Any?): Boolean {{\n        if (this === other) return true\n        if (other == null || other::class != this::class) return false\n        other as {name}\n        return id != null && id == other.id\n    }}\n\n    override fun hashCode(): Int = this::class.hashCode()"
        );
    }

    format!(
        "    @Override\n    public boolean equals(Object o) {{\n        if (this == o) {{\n            return true;\n        }}\n        if (o == null || getClass() != o.getClass()) {{\n            return false;\n        }}\n        {name} other = ({name}) o;\n        return id != null && id.equals(other.id);\n    }}\n\n    @Override\n    public int hashCode() {{\n        return getClass().hashCode();\n    }}"
    )
}

/// The field (and its imports) at the end of the class body; `None` when the class has no body.
pub fn attribute_edits(class: &Class, text: &str, masked: &str, spec: &AttributeSpec) -> Option<Vec<Edit>> {
    let at = class.body_end?;
    let kotlin = class.kotlin;
    let mut imports: Vec<String> = Vec::new();
    let mut annotations = Vec::new();
    let mut ty = spec.ty.clone();
    let mut init = None;

    let mut annotate = |annotation: &str, args: Vec<String>, imports: &mut Vec<String>| {
        imports.push(callback_import(class, annotation));
        annotations.push(if args.is_empty() { format!("@{annotation}") } else { format!("@{annotation}({})", args.join(", ")) });
    };

    if spec.relation == Relation::None {
        let mut args = Vec::new();
        if spec.not_null {
            args.push("nullable = false".to_owned());
        }
        if spec.unique {
            args.push("unique = true".to_owned());
        }
        if !args.is_empty() {
            annotate("Column", args, &mut imports);
        }
        if spec.enumerated {
            imports.push(callback_import(class, "EnumType"));
            annotate("Enumerated", vec!["EnumType.STRING".into()], &mut imports);
        }
        imports.extend(type_import(&ty).map(str::to_owned));
    } else {
        let mut args = Vec::new();
        if spec.fetch {
            imports.push(callback_import(class, "FetchType"));
            let kind = if spec.relation.to_one() { "LAZY" } else { "EAGER" };
            args.push(format!("fetch = FetchType.{kind}"));
        }
        if spec.cascade {
            imports.push(callback_import(class, "CascadeType"));
            args.push("cascade = CascadeType.ALL".into());
        }
        if spec.not_null && spec.relation.to_one() {
            args.push("optional = false".into());
        }
        let relation = format!("{:?}", spec.relation);
        annotate(&relation, args, &mut imports);

        if spec.relation.to_many() {
            ty = if kotlin { format!("MutableList<{ty}>") } else { format!("List<{ty}>") };
            init = Some(if kotlin { "mutableListOf()" } else { "new ArrayList<>()" }.to_owned());
            if !kotlin {
                imports.extend(["java.util.List".to_owned(), "java.util.ArrayList".to_owned()]);
            }
        }
    }

    if kotlin {
        ty = kotlin_type(&ty).to_owned();
    }

    let accessors = class.attributes.iter().any(|a| crate::model::has_word(masked, &format!("get{}", capitalize(&a.name))));
    let field = Field { annotations, ty, name: spec.name.clone(), init };
    let mut edits: Vec<Edit> = {
        let refs: Vec<&str> = imports.iter().map(String::as_str).collect();
        import_edit(text, &refs, kotlin).into_iter().collect()
    };
    edits.push(Edit::insert(at, &format!("\n{}\n", render(&field, kotlin, accessors))));

    if text.contains("\r\n") {
        edits.iter_mut().for_each(|e| e.text = e.text.replace('\n', "\r\n"));
    }

    Some(edits)
}
