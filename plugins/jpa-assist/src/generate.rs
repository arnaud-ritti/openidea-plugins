//! Code and SQL written from entities: equals/hashCode/toString, DDL, DTOs,
//! and entities from `CREATE TABLE` statements.

use crate::model::{Attribute, Class, Edit, Kind, Relation, camel_case, capitalize, closing, decapitalize, snake_case};
use crate::queries::convert_type;

/// The classes known by simple name.
pub type Known<'a> = &'a dyn Fn(&str) -> Option<&'a Class>;

/// `class`'s attributes after those of its superclasses.
pub fn inherited(class: &Class, known: Known) -> Vec<Attribute> {
    let mut chain = vec![class];

    while let Some(parent) = chain.last().and_then(|c| c.superclass.as_deref()).and_then(known) {
        if chain.len() > 8 || chain.iter().any(|c| c.name == parent.name) {
            break;
        }
        chain.push(parent);
    }

    chain.iter().rev().flat_map(|c| c.attributes.iter().cloned()).collect()
}

fn id_attribute(class: &Class, known: Known) -> Option<Attribute> {
    inherited(class, known).into_iter().find(|a| a.id)
}

/// `equals` and `hashCode` comparing the id, the same for proxies of the entity.
pub fn equals_and_hash_code(class: &Class, text: &str, known: Known) -> Option<Edit> {
    let at = class.body_end?;
    let id = id_attribute(class, known).map_or_else(|| "id".to_owned(), |a| a.name);
    let id = id.as_str();
    let name = &class.name;

    let method = if class.kotlin {
        format!(
            "
    override fun equals(other: Any?): Boolean {{
        if (this === other) return true
        if (other !is {name}) return false
        return {id} != null && {id} == other.{id}
    }}

    override fun hashCode(): Int = {name}::class.hashCode()
"
        )
    } else {
        // A proxy's fields are never loaded: read the other side through its getter when there is one.
        let getter = format!("get{}()", capitalize(id));
        let has_getter = text.contains(&format!("{getter} {{")) || text.contains("@Getter") || text.contains("@Data");
        let other_id = if has_getter { format!("other.{getter}") } else { format!("other.{id}") };
        format!(
            "
    @Override
    public boolean equals(Object o) {{
        if (this == o) {{
            return true;
        }}
        if (!(o instanceof {name} other)) {{
            return false;
        }}
        return {id} != null && {id}.equals({other_id});
    }}

    @Override
    public int hashCode() {{
        return {name}.class.hashCode();
    }}
"
        )
    };

    Some(Edit::insert(at, &method))
}

/// The annotation's class: `javax` when the file already imports `javax.persistence`.
pub fn callback_import(class: &Class, annotation: &str) -> String {
    let javax = class.imports.iter().any(|i| i.starts_with("javax.persistence"));
    format!("{}.persistence.{annotation}", if javax { "javax" } else { "jakarta" })
}

/// An empty lifecycle callback method at the end of the class body.
pub fn callback_method(class: &Class, annotation: &str) -> Option<Edit> {
    let at = class.body_end?;
    let name = decapitalize(annotation);
    let keyword = if class.kotlin { "fun" } else { "void" };

    Some(Edit::insert(at, &format!("\n    @{annotation} {keyword} {name}() {{}}\n")))
}

/// `toString` over the id and basic attributes: it never touches a lazy association.
pub fn to_string_method(class: &Class, known: Known) -> Option<Edit> {
    let at = class.body_end?;
    let shown: Vec<String> = inherited(class, known)
        .into_iter()
        .filter(|a| a.relation == Relation::None && !a.lob)
        .map(|a| a.name)
        .collect();

    let method = if class.kotlin {
        let fields: Vec<String> = shown.iter().map(|n| format!("{n} = ${n}")).collect();
        format!("\n    override fun toString(): String = \"{}({})\"\n", class.name, fields.join(", "))
    } else {
        let fields: String = shown
            .iter()
            .enumerate()
            .map(|(i, n)| format!("\"{}{n} = \" + {n} + ", if i == 0 { "(" } else { ", " }))
            .collect();
        let open = if fields.is_empty() { "\"(" } else { "\"" };
        format!(
            "
    @Override
    public String toString() {{
        return getClass().getSimpleName() + {fields}{open})\";
    }}
"
        )
    };

    Some(Edit::insert(at, &method))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    PostgreSql,
    MySql,
}

impl Dialect {
    pub fn from_setting(value: Option<&str>) -> Self {
        if value == Some("mysql") { Self::MySql } else { Self::PostgreSql }
    }
}

/// Spring Boot's default naming: snake case, explicit names included.
pub fn table_name(class: &Class) -> String {
    snake_case(class.table.as_deref().unwrap_or(&class.name))
}

fn column_name(a: &Attribute) -> String {
    snake_case(a.column.as_deref().unwrap_or(&a.name))
}

fn base_type(ty: &str) -> &str {
    ty.trim_end_matches('?').rsplit('.').next().unwrap_or(ty)
}

fn sql_type(a: &Attribute, dialect: Dialect) -> Option<String> {
    let pg = dialect == Dialect::PostgreSql;
    let pick = |p: &str, m: &str| Some(if pg { p } else { m }.to_owned());

    match base_type(&a.ty) {
        "Long" | "long" => pick("BIGINT", "BIGINT"),
        "Integer" | "int" | "Int" => pick("INTEGER", "INT"),
        "Short" | "short" => pick("SMALLINT", "SMALLINT"),
        "Byte" | "byte" => pick("SMALLINT", "TINYINT"),
        "Boolean" | "boolean" => pick("BOOLEAN", "BIT(1)"),
        "Double" | "double" => pick("DOUBLE PRECISION", "DOUBLE"),
        "Float" | "float" => pick("REAL", "FLOAT"),
        "BigDecimal" => {
            let size = format!("({}, {})", a.precision.unwrap_or(38), a.scale.unwrap_or(2));
            pick(&format!("NUMERIC{size}"), &format!("DECIMAL{size}"))
        }
        "BigInteger" => pick("NUMERIC(38)", "DECIMAL(38)"),
        "String" if a.lob => pick("TEXT", "LONGTEXT"),
        "String" => Some(format!("VARCHAR({})", a.length.unwrap_or(255))),
        "Character" | "char" | "Char" => pick("CHAR(1)", "CHAR(1)"),
        "LocalDate" => pick("DATE", "DATE"),
        "LocalTime" => pick("TIME", "TIME"),
        "LocalDateTime" | "Date" | "Timestamp" => pick("TIMESTAMP", "DATETIME(6)"),
        "Instant" | "OffsetDateTime" | "ZonedDateTime" => pick("TIMESTAMP WITH TIME ZONE", "DATETIME(6)"),
        "UUID" => pick("UUID", "BINARY(16)"),
        "byte[]" | "Byte[]" | "ByteArray" => pick("BYTEA", "LONGBLOB"),
        _ => match a.enum_string {
            Some(true) => pick("VARCHAR(255)", "VARCHAR(255)"),
            Some(false) => pick("SMALLINT", "TINYINT"),
            None => None,
        },
    }
}

struct Table {
    name: String,
    columns: Vec<String>,
    primary_key: Vec<String>,
    unique: Vec<String>,
}

impl Table {
    fn new(name: String) -> Self {
        Self { name, columns: Vec::new(), primary_key: Vec::new(), unique: Vec::new() }
    }

    fn sql(&self) -> String {
        let mut lines = self.columns.clone();

        if !self.primary_key.is_empty() {
            lines.push(format!("CONSTRAINT {}_pkey PRIMARY KEY ({})", self.name, self.primary_key.join(", ")));
        }

        for column in &self.unique {
            lines.push(format!("CONSTRAINT {}_{column}_key UNIQUE ({column})", self.name));
        }

        format!("CREATE TABLE {}\n(\n    {}\n);\n", self.name, lines.join(",\n    "))
    }
}

/// The id column of `class` and its SQL type.
fn key_of(class: Option<&Class>, known: Known, dialect: Dialect) -> (String, String) {
    let id = class.and_then(|c| id_attribute(c, known));
    let column = id.as_ref().map_or("id".to_owned(), column_name);
    let ty = id.as_ref().and_then(|a| sql_type(a, dialect)).unwrap_or_else(|| "BIGINT".into());
    (column, ty)
}

fn foreign_key(table: &str, column: &str, target_table: &str, target_column: &str) -> String {
    format!("ALTER TABLE {table}\n    ADD CONSTRAINT {table}_{column}_fkey FOREIGN KEY ({column}) REFERENCES {target_table} ({target_column});\n")
}

/// `CREATE` statements for `entities`, foreign keys last so the order of tables doesn't matter.
pub fn ddl(entities: &[&Class], known: Known, dialect: Dialect) -> String {
    let mut sequences = Vec::new();
    let mut tables = Vec::new();
    let mut constraints = Vec::new();

    for entity in entities.iter().filter(|e| e.kind == Kind::Entity) {
        let mut table = Table::new(table_name(entity));
        let mut join_tables = Vec::new();

        for a in inherited(entity, known) {
            let target = known(&a.target);
            let target_table = target.map_or_else(|| snake_case(&a.target), table_name);

            match a.relation {
                Relation::None | Relation::Embedded => {
                    let parts: Vec<&Attribute> = match (a.relation, target) {
                        (Relation::Embedded, Some(embeddable)) => embeddable.attributes.iter().collect(),
                        (Relation::Embedded, None) => {
                            table.columns.push(format!("-- {}: embeddable {} not found", a.name, a.target));
                            continue;
                        }
                        _ => vec![&a],
                    };

                    for part in parts {
                        let column = column_name(part);
                        let Some(ty) = sql_type(part, dialect) else {
                            table.columns.push(format!("-- {column}: no SQL type for {}", part.ty));
                            continue;
                        };

                        let numeric = matches!(base_type(&part.ty), "Long" | "long" | "Integer" | "int" | "Int");
                        let generated = part.generated.as_deref().filter(|_| numeric && a.id);
                        let definition = match (generated, dialect) {
                            (Some("IDENTITY"), Dialect::PostgreSql) => format!("{column} {ty} GENERATED BY DEFAULT AS IDENTITY"),
                            (Some("IDENTITY"), Dialect::MySql) => format!("{column} {ty} NOT NULL AUTO_INCREMENT"),
                            _ if a.id || part.not_null => format!("{column} {ty} NOT NULL"),
                            _ => format!("{column} {ty}"),
                        };

                        if let Some(strategy) = generated.filter(|s| *s != "IDENTITY") {
                            let sequence = format!("{}_seq", snake_case(&entity.name));
                            sequences.push(match dialect {
                                Dialect::PostgreSql => format!("CREATE SEQUENCE IF NOT EXISTS {sequence} START WITH 1 INCREMENT BY 50;\n"),
                                Dialect::MySql => format!("-- {strategy} ids: Hibernate keeps the next value in table {sequence}\nCREATE TABLE {sequence}\n(\n    next_val BIGINT\n);\nINSERT INTO {sequence} VALUES (1);\n"),
                            });
                        }

                        table.columns.push(definition);
                        if a.id {
                            table.primary_key.push(column.clone());
                        }
                        if part.unique {
                            table.unique.push(column);
                        }
                    }
                }
                Relation::ManyToOne | Relation::OneToOne if !a.mapped_by => {
                    let (target_column, ty) = key_of(target, known, dialect);
                    let column = a.column.as_deref().map_or_else(|| format!("{}_{target_column}", snake_case(&a.name)), snake_case);
                    let null = if a.not_null || a.id { " NOT NULL" } else { "" };
                    table.columns.push(format!("{column} {ty}{null}"));

                    if a.id {
                        table.primary_key.push(column.clone());
                    }
                    if a.relation == Relation::OneToOne {
                        table.unique.push(column.clone());
                    }
                    constraints.push(foreign_key(&table.name, &column, &target_table, &target_column));
                }
                Relation::OneToMany | Relation::ManyToMany if !a.mapped_by && !a.join_column => {
                    let (own_column, own_type) = key_of(Some(entity), known, dialect);
                    let (target_column, target_type) = key_of(target, known, dialect);
                    let join = a.join_table.as_deref().map_or_else(|| format!("{}_{}", snake_case(&entity.name), snake_case(&a.target)), snake_case);
                    let owner = format!("{}_{own_column}", snake_case(&entity.name));
                    let element = format!("{}_{target_column}", snake_case(&a.name));

                    let mut join_table = Table::new(join.clone());
                    join_table.columns = vec![format!("{owner} {own_type} NOT NULL"), format!("{element} {target_type} NOT NULL")];
                    join_table.primary_key = vec![owner.clone(), element.clone()];
                    if a.relation == Relation::OneToMany {
                        join_table.unique.push(element.clone());
                    }

                    join_tables.push(join_table.sql());
                    constraints.push(foreign_key(&join, &owner, &table.name, &own_column));
                    constraints.push(foreign_key(&join, &element, &target_table, &target_column));
                }
                Relation::OneToMany if !a.mapped_by => {
                    table.columns.push(format!("-- {}: its join column lives in table {target_table}", a.name));
                }
                Relation::ElementCollection => {
                    table.columns.push(format!("-- {}: element collections aren't generated", a.name));
                }
                _ => {}
            }
        }

        tables.push(table.sql());
        tables.extend(join_tables);
    }

    sequences.sort();
    sequences.dedup();
    [sequences, tables, constraints].concat().join("\n")
}

/// A record (Java) or data class (Kotlin) with the entity's basic attributes and the ids of its to-one associations.
pub fn dto(class: &Class, known: Known) -> String {
    let mut imports = Vec::new();
    let mut fields = Vec::new();

    for a in inherited(class, known) {
        let (name, ty) = match a.relation {
            Relation::None | Relation::Embedded => (a.name.clone(), a.ty.clone()),
            Relation::ManyToOne | Relation::OneToOne => {
                let id = known(&a.target).and_then(|t| id_attribute(t, known));
                let id_name = id.as_ref().map_or("id", |i| i.name.as_str());
                (format!("{}{}", a.name, capitalize(id_name)), id.as_ref().map_or_else(|| "Long".into(), |i| i.ty.clone()))
            }
            _ => continue,
        };

        let ty = convert_type(&ty, class.kotlin);
        imports.extend(class.qualified(base_type(&ty)));
        fields.push((name, ty));
    }

    imports.sort();
    imports.dedup();
    let semicolon = if class.kotlin { "" } else { ";" };
    let mut out = class.package.as_ref().map(|p| format!("package {p}{semicolon}\n\n")).unwrap_or_default();

    for import in imports.iter().filter(|i| class.package.as_ref().is_none_or(|p| i.rsplit_once('.').map(|(pk, _)| pk) != Some(p))) {
        out.push_str(&format!("import {import}{semicolon}\n"));
    }

    if !imports.is_empty() {
        out.push('\n');
    }

    let name = format!("{}Dto", class.name);
    if class.kotlin {
        let params: Vec<String> = fields.iter().map(|(n, t)| format!("    val {n}: {t}? = null,")).collect();
        out.push_str(&format!("data class {name}(\n{}\n)\n", params.join("\n")));
    } else {
        let params: Vec<String> = fields.iter().map(|(n, t)| format!("{t} {n}")).collect();
        out.push_str(&format!("public record {name}(\n        {}\n) {{\n}}\n", params.join(",\n        ")));
    }

    out
}

/// SQL with comments and literals blanked (same offsets); `"quoted"` identifiers are kept.
fn mask_sql(sql: &str) -> String {
    let b = sql.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;

    while i < b.len() {
        let end = if b[i..].starts_with(b"--") {
            b[i..].iter().position(|&c| c == b'\n').map_or(b.len(), |p| i + p)
        } else if b[i..].starts_with(b"/*") {
            b[i + 2..].windows(2).position(|w| w == b"*/").map_or(b.len(), |p| i + 2 + p + 2)
        } else if b[i] == b'\'' {
            let mut j = i + 1;
            while j < b.len() && !(b[j] == b'\'' && b.get(j + 1) != Some(&b'\'')) {
                j += if b[j] == b'\'' { 2 } else { 1 };
            }
            (j + 1).min(b.len())
        } else {
            i += 1;
            continue;
        };

        for c in &mut out[i..end] {
            if *c != b'\n' {
                *c = b' ';
            }
        }
        i = end;
    }

    String::from_utf8(out).expect("whole characters are replaced by ASCII spaces")
}

fn unquote(name: &str) -> String {
    let last = name.rsplit('.').next().unwrap_or(name);
    last.trim_matches(['"', '`', '[', ']']).to_owned()
}

fn split_parens(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0);

    for (i, c) in s.bytes().enumerate() {
        match c {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }

    parts.push(&s[start..]);
    parts
}

fn column_list(s: &str) -> Vec<String> {
    let inner = s.split_once('(').and_then(|(_, r)| r.split_once(')')).map_or("", |(cols, _)| cols);
    inner.split(',').map(|c| unquote(c.trim())).filter(|c| !c.is_empty()).collect()
}

struct Column {
    name: String,
    java: &'static str,
    kotlin: &'static str,
    import: Option<&'static str>,
    length: Option<u32>,
    precision: Option<(u32, u32)>,
    not_null: bool,
    generated: bool,
    primary: bool,
    references: Option<String>,
}

fn column(definition: &str) -> Option<Column> {
    let mut tokens = definition.split_whitespace();
    let name = unquote(tokens.next()?);
    let rest: String = tokens.collect::<Vec<_>>().join(" ");
    let lower = rest.to_lowercase();
    let base: String = lower.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    let size: Vec<u32> = lower
        .split_once('(')
        .and_then(|(_, r)| r.split_once(')'))
        .map(|(s, _)| s.split(',').filter_map(|n| n.trim().parse().ok()).collect())
        .unwrap_or_default();

    let ((java, kotlin), import) = match base.as_str() {
        "bigint" | "int8" | "bigserial" | "serial8" => (("Long", "Long"), None),
        "int" | "integer" | "int4" | "serial" | "serial4" | "mediumint" => (("Integer", "Int"), None),
        "tinyint" if size == [1] => (("Boolean", "Boolean"), None),
        "smallint" | "int2" | "smallserial" | "tinyint" => (("Short", "Short"), None),
        "bool" | "boolean" | "bit" => (("Boolean", "Boolean"), None),
        "numeric" | "decimal" | "number" | "money" => (("BigDecimal", "BigDecimal"), Some("java.math.BigDecimal")),
        "real" | "float4" => (("Float", "Float"), None),
        "double" | "float8" | "float" => (("Double", "Double"), None),
        "date" => (("LocalDate", "LocalDate"), Some("java.time.LocalDate")),
        "timestamptz" => (("OffsetDateTime", "OffsetDateTime"), Some("java.time.OffsetDateTime")),
        "timestamp" if lower.contains("with time zone") => (("OffsetDateTime", "OffsetDateTime"), Some("java.time.OffsetDateTime")),
        "timestamp" | "datetime" => (("LocalDateTime", "LocalDateTime"), Some("java.time.LocalDateTime")),
        "time" | "timetz" => (("LocalTime", "LocalTime"), Some("java.time.LocalTime")),
        "uuid" => (("UUID", "UUID"), Some("java.util.UUID")),
        "bytea" | "blob" | "longblob" | "mediumblob" | "binary" | "varbinary" => (("byte[]", "ByteArray"), None),
        _ => (("String", "String"), None),
    };

    let textual = matches!(base.as_str(), "varchar" | "character" | "char" | "nvarchar" | "nchar" | "varchar2");
    let references = lower.find("references ").map(|i| unquote(rest[i + "references ".len()..].split(['(', ' ']).next().unwrap_or("")));

    Some(Column {
        name,
        java,
        kotlin,
        import,
        length: size.first().copied().filter(|_| textual),
        precision: (java == "BigDecimal" && !size.is_empty()).then(|| (size[0], size.get(1).copied().unwrap_or(0))),
        not_null: lower.contains("not null") || lower.contains("primary key"),
        primary: lower.contains("primary key"),
        generated: lower.contains("serial") || lower.contains("auto_increment") || lower.contains("identity") || lower.contains("generated"),
        references,
    })
}

/// The entity for the `CREATE TABLE` statement around `at` in `sql`.
pub fn entity_from_ddl(sql: &str, at: usize, kotlin: bool) -> Option<String> {
    let masked = mask_sql(sql);
    let lower = masked.to_lowercase();
    let start = lower[..at.min(lower.len())].rfind(';').map_or(0, |p| p + 1);
    let end = lower[start..].find(';').map_or(lower.len(), |p| p + start);
    let create = lower[start..end].find("create table").map(|p| p + start)?;

    let mut after = &masked[create + "create table".len()..end];
    for skip in ["if not exists"] {
        if after.trim_start().to_lowercase().starts_with(skip) {
            after = &after.trim_start()[skip.len()..];
        }
    }

    let open = after.find('(')?;
    let table = unquote(after[..open].trim());
    let body_start = end - after.len() + open;
    let body_end = closing(masked.as_bytes(), body_start)?;

    let mut columns = Vec::new();
    let mut primary_key = Vec::new();
    let mut foreign: Vec<(String, String)> = Vec::new();

    for item in split_parens(&masked[body_start + 1..body_end]) {
        let item = item.trim();
        let low = item.to_lowercase();
        let constraint = low.strip_prefix("constraint").map(|r| r.trim_start().split_once(char::is_whitespace).map_or("", |(_, r)| r.trim_start()));
        let clause = constraint.unwrap_or(&low);

        if clause.starts_with("primary key") {
            primary_key = column_list(clause);
        } else if clause.starts_with("foreign key") {
            let referenced = clause.split_once("references").map(|(_, r)| unquote(r.trim().split(['(', ' ']).next().unwrap_or(""))).unwrap_or_default();
            foreign.extend(column_list(clause).into_iter().map(|c| (c, referenced.clone())));
        } else if !["unique", "key", "index", "check", "exclude", "fulltext"].iter().any(|k| clause.starts_with(k)) && constraint.is_none() {
            columns.extend(column(item));
        }
    }

    for c in &mut columns {
        if primary_key.iter().any(|k| k.eq_ignore_ascii_case(&c.name)) {
            c.not_null = true;
        }
        if c.references.is_none() {
            c.references = foreign.iter().find(|(col, _)| col.eq_ignore_ascii_case(&c.name)).map(|(_, t)| t.clone());
        }
    }

    let key = match primary_key.as_slice() {
        [] => columns.iter().position(|c| c.primary),
        [single] => columns.iter().position(|c| c.name.eq_ignore_ascii_case(single)),
        _ => None,
    };

    Some(render_entity(&table, &columns, key, primary_key.len() > 1, kotlin))
}

fn render_entity(table: &str, columns: &[Column], key: Option<usize>, composite: bool, kotlin: bool) -> String {
    let semicolon = if kotlin { "" } else { ";" };
    let mut imports: Vec<&str> = vec!["jakarta.persistence.*"];
    imports.extend(columns.iter().filter(|c| c.references.is_none()).filter_map(|c| c.import));
    imports.sort();
    imports.dedup();

    let mut out: String = imports.iter().map(|i| format!("import {i}{semicolon}\n")).collect();
    let class = camel_case(table, true);
    out.push_str(&format!("\n@Entity\n@Table(name = \"{table}\")\n"));
    out.push_str(&if kotlin { format!("class {class} {{\n") } else { format!("public class {class} {{\n") });

    if composite {
        out.push_str("    // Composite primary key: map it with an @EmbeddedId.\n");
    } else if key.is_none() {
        out.push_str("    // No primary key: JPA needs an @Id.\n");
    }

    let mut accessors = Vec::new();
    for (i, c) in columns.iter().enumerate() {
        let mut lines = Vec::new();
        let (field, java, kotlin_type) = match &c.references {
            Some(target) => {
                let class = camel_case(target, true);
                let field = camel_case(c.name.trim_end_matches("_id").trim_end_matches("_ID"), false);
                lines.push("@ManyToOne(fetch = FetchType.LAZY)".to_owned());
                let null = if c.not_null { ", nullable = false" } else { "" };
                lines.push(format!("@JoinColumn(name = \"{}\"{null})", c.name));
                (field, class.clone(), class)
            }
            None => {
                if Some(i) == key {
                    lines.push("@Id".to_owned());
                    if c.generated {
                        lines.push("@GeneratedValue(strategy = GenerationType.IDENTITY)".to_owned());
                    }
                }
                let mut args = vec![format!("name = \"{}\"", c.name)];
                if c.not_null {
                    args.push("nullable = false".into());
                }
                if let Some(length) = c.length.filter(|&l| l != 255) {
                    args.push(format!("length = {length}"));
                }
                if let Some((precision, scale)) = c.precision {
                    args.push(format!("precision = {precision}"));
                    args.push(format!("scale = {scale}"));
                }
                lines.push(format!("@Column({})", args.join(", ")));
                (camel_case(&c.name, false), c.java.to_owned(), c.kotlin.to_owned())
            }
        };

        let declaration = if kotlin { format!("var {field}: {kotlin_type}? = null") } else { format!("private {java} {field};") };
        lines.push(declaration);
        out.push_str(&lines.iter().map(|l| format!("    {l}\n")).collect::<String>());
        out.push('\n');

        if !kotlin {
            let cap = capitalize(&field);
            accessors.push(format!(
                "    public {java} get{cap}() {{\n        return {field};\n    }}\n\n    public void set{cap}({java} {field}) {{\n        this.{field} = {field};\n    }}\n"
            ));
        }
    }

    if kotlin {
        out.truncate(out.trim_end().len());
        out.push('\n');
    } else {
        out.push_str(&accessors.join("\n"));
    }

    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::parse_class;

    const ORDER: &str = r#"package shop;

import jakarta.persistence.*;
import java.math.BigDecimal;

@Entity
@Table(name = "purchase_orders")
public class Order {
    @Id
    @GeneratedValue(strategy = GenerationType.IDENTITY)
    private Long id;

    @Column(name = "order_number", nullable = false, length = 20, unique = true)
    private String number;

    @Column(precision = 10, scale = 2)
    private BigDecimal total;

    @ManyToOne(fetch = FetchType.LAZY, optional = false)
    private Customer customer;

    @ManyToMany
    private Set<Product> products;

    @OneToMany(mappedBy = "order")
    private List<OrderLine> lines;

    public Long getId() {
        return id;
    }
}
"#;

    const CUSTOMER: &str = "package shop\n\n@Entity\nclass Customer {\n    @Id\n    @GeneratedValue\n    var code: Int? = null\n    var name: String? = null\n}\n";

    fn classes() -> Vec<Class> {
        vec![parse_class(ORDER, "/p/Order.java").unwrap(), parse_class(CUSTOMER, "/p/Customer.kt").unwrap()]
    }

    #[test]
    fn writes_ddl_with_keys_sequences_and_join_tables() {
        let classes = classes();
        let known = |n: &str| classes.iter().find(|c| c.name == n);
        let sql = ddl(&[&classes[0], &classes[1]], &known, Dialect::PostgreSql);

        assert!(sql.starts_with("CREATE SEQUENCE IF NOT EXISTS customer_seq START WITH 1 INCREMENT BY 50;\n"), "{sql}");
        assert!(sql.contains(
            "CREATE TABLE purchase_orders\n(\n    id BIGINT GENERATED BY DEFAULT AS IDENTITY,\n    order_number VARCHAR(20) NOT NULL,\n    total NUMERIC(10, 2),\n    customer_code INTEGER NOT NULL,\n    CONSTRAINT purchase_orders_pkey PRIMARY KEY (id),\n    CONSTRAINT purchase_orders_order_number_key UNIQUE (order_number)\n);\n"
        ), "{sql}");
        assert!(sql.contains("CREATE TABLE order_product\n(\n    order_id BIGINT NOT NULL,\n    products_id BIGINT NOT NULL,\n    CONSTRAINT order_product_pkey PRIMARY KEY (order_id, products_id)\n);\n"), "{sql}");
        assert!(sql.contains("ALTER TABLE purchase_orders\n    ADD CONSTRAINT purchase_orders_customer_code_fkey FOREIGN KEY (customer_code) REFERENCES customer (code);\n"), "{sql}");
        assert!(sql.contains("REFERENCES product (id)"));
        assert!(!sql.contains("lines"));

        let mysql = ddl(&[&classes[0]], &known, Dialect::MySql);
        assert!(mysql.contains("id BIGINT NOT NULL AUTO_INCREMENT,"), "{mysql}");
    }

    #[test]
    fn writes_equals_hash_code_and_to_string() {
        let classes = classes();
        let known = |n: &str| classes.iter().find(|c| c.name == n);
        let equals = equals_and_hash_code(&classes[0], ORDER, &known).unwrap();
        assert_eq!(&ORDER[equals.start..], "}\n");
        assert!(equals.text.contains("if (!(o instanceof Order other)) {"));
        assert!(equals.text.contains("return id != null && id.equals(other.getId());"));
        assert!(equals.text.contains("return Order.class.hashCode();"));

        let kotlin = equals_and_hash_code(&classes[1], CUSTOMER, &known).unwrap();
        assert!(kotlin.text.contains("return code != null && code == other.code"));

        let to_string = to_string_method(&classes[0], &known).unwrap();
        assert!(to_string.text.contains(
            "return getClass().getSimpleName() + \"(id = \" + id + \", number = \" + number + \", total = \" + total + \")\";"
        ), "{}", to_string.text);
        assert_eq!(to_string_method(&classes[1], &known).unwrap().text, "\n    override fun toString(): String = \"Customer(code = $code, name = $name)\"\n");
    }

    #[test]
    fn writes_a_dto() {
        let classes = classes();
        let known = |n: &str| classes.iter().find(|c| c.name == n);
        assert_eq!(
            dto(&classes[0], &known),
            "package shop;\n\nimport java.math.BigDecimal;\n\npublic record OrderDto(\n        Long id,\n        String number,\n        BigDecimal total,\n        Integer customerCode\n) {\n}\n"
        );
        assert_eq!(dto(&classes[1], &known), "package shop\n\ndata class CustomerDto(\n    val code: Int? = null,\n    val name: String? = null,\n)\n");
    }

    const SQL: &str = "-- schema
CREATE TABLE IF NOT EXISTS public.order_line (
    id bigserial PRIMARY KEY,
    product_name varchar(80) NOT NULL, -- shown on invoices
    unit_price numeric(10, 2),
    created_at timestamp with time zone,
    order_id bigint REFERENCES purchase_orders (id),
    CONSTRAINT uq UNIQUE (product_name)
);

CREATE TABLE tag (a int, b int, PRIMARY KEY (a, b));
";

    #[test]
    fn writes_an_entity_from_create_table() {
        let java = entity_from_ddl(SQL, SQL.find("unit_price").unwrap(), false).unwrap();
        assert!(java.starts_with("import jakarta.persistence.*;\nimport java.math.BigDecimal;\nimport java.time.OffsetDateTime;\n\n@Entity\n@Table(name = \"order_line\")\npublic class OrderLine {\n"), "{java}");
        assert!(java.contains("    @Id\n    @GeneratedValue(strategy = GenerationType.IDENTITY)\n    @Column(name = \"id\", nullable = false)\n    private Long id;\n"), "{java}");
        assert!(java.contains("    @Column(name = \"product_name\", nullable = false, length = 80)\n    private String productName;\n"), "{java}");
        assert!(java.contains("    @Column(name = \"unit_price\", precision = 10, scale = 2)\n    private BigDecimal unitPrice;\n"));
        assert!(java.contains("    @ManyToOne(fetch = FetchType.LAZY)\n    @JoinColumn(name = \"order_id\")\n    private PurchaseOrders order;\n"), "{java}");
        assert!(java.contains("    public void setProductName(String productName) {\n        this.productName = productName;\n    }\n"));

        let kotlin = entity_from_ddl(SQL, SQL.find("tag").unwrap(), true).unwrap();
        assert!(kotlin.contains("class Tag {\n    // Composite primary key: map it with an @EmbeddedId.\n"), "{kotlin}");
        assert!(kotlin.ends_with("    @Column(name = \"b\", nullable = false)\n    var b: Int? = null\n}\n"), "{kotlin}");
        assert!(entity_from_ddl("select 1;", 3, false).is_none());
    }
}
