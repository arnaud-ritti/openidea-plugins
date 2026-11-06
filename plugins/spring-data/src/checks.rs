//! Text checks over Spring Data repositories and JDBC entities. A check stays
//! quiet when an annotation or type comes from a library other than Spring or
//! a value is not a plain literal.

use crate::{derived, derived_method};
use openidea_plugin::{Severity, host};
use spring_text::finding::Finding;
use spring_text::java::{Annotation, JavaFile, Member, TypeRef};

const PAGE_RETURNS: [&str; 2] = ["Page", "Slice"];
const PAGING_SUFFIXES: [&str; 2] = ["Pageable", "PageRequest"];
const WRITING_KEYWORDS: [&str; 2] = ["update", "delete"];
const KEYED_COLLECTIONS: [&str; 2] = ["List", "Map"];

/// Which groups of checks run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Groups {
    pub pageable: bool,
    pub modifying: bool,
    pub jdbc_mapping: bool,
}

impl Groups {
    pub fn from_settings() -> Self {
        let on = |key: &str| host::setting_bool(&format!("springData.{key}")).unwrap_or(true);
        Self { pageable: on("checkPageable"), modifying: on("checkModifying"), jdbc_mapping: on("checkJdbcMapping") }
    }
}

/// `repository_types`: indexes into `file.types` of the repository interfaces; the JDBC mapping checks cover every type.
pub fn check(file: &JavaFile, repository_types: &[usize], groups: Groups) -> Vec<Finding> {
    let mut out = Vec::new();
    for &index in repository_types {
        for method in file.types[index].methods() {
            if groups.pageable {
                page_without_pageable(file, method, &mut out);
            }
            if groups.modifying {
                query_needs_modifying(file, method, &mut out);
            }
        }
    }
    if groups.jdbc_mapping {
        for ty in &file.types {
            let properties = ty
                .fields()
                .filter_map(|f| f.ty.as_ref().map(|t| (f.annotations.as_slice(), t)))
                .chain(ty.components.iter().map(|c| (c.annotations.as_slice(), &c.ty)));
            for (annotations, property_type) in properties {
                for annotation in annotations {
                    match annotation.name.as_str() {
                        "Sequence" if file.from_spring("Sequence") => empty_sequence_name(annotation, &mut out),
                        "MappedCollection" if file.from_spring("MappedCollection") => {
                            mapped_collection_key_column(annotation, property_type, &mut out);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    out
}

fn abstract_method(method: &Member) -> bool {
    method.body.is_none() && !method.modifiers.iter().any(|m| m == "default" || m == "static")
}

fn single_literal(annotation: &Annotation, name: &str) -> Option<(String, usize, usize)> {
    let arg = annotation.arg(name)?;
    let [literal] = arg.strings.as_slice() else { return None };
    let quotes = arg.text.len().checked_sub(literal.end - literal.at)?;
    let delimited = arg.text.starts_with('"') && arg.text.ends_with('"') && (quotes == 2 || quotes == 6);
    delimited.then(|| (literal.value.clone(), arg.at, arg.end))
}

/// A `Page` or `Slice` result needs a `Pageable` to know which page to read; derived and `@Query` methods have no other source for it.
fn page_without_pageable(file: &JavaFile, method: &Member, out: &mut Vec<Finding>) {
    let Some(result) = &method.ty else { return };
    if !PAGE_RETURNS.contains(&result.name.as_str()) || !file.from_spring(&result.name) {
        return;
    }
    let is_query = method.has(&["Query", "NativeQuery"]) && abstract_method(method);
    let is_derived = derived_method(method) && derived(&method.name).is_some();
    if !is_query && !is_derived {
        return;
    }
    let paged = method.params.iter().any(|p| PAGING_SUFFIXES.iter().any(|s| p.ty.name.ends_with(s)));
    if paged {
        return;
    }
    out.push(Finding::new(
        method.at,
        method.at + method.name.len(),
        "page-without-pageable",
        format!("A method returning {} needs a Pageable parameter that says which page to read. Add `Pageable pageable`.", result.name),
    ));
}

/// A `@Query` that starts with `update` or `delete` runs as a read unless `@Modifying` marks it.
fn query_needs_modifying(file: &JavaFile, method: &Member, out: &mut Vec<Finding>) {
    let Some(query) = ["Query", "NativeQuery"].iter().find_map(|n| method.annotation(n)) else { return };
    if method.has(&["Modifying", "Procedure"]) || !file.from_spring(&query.name) || !abstract_method(method) {
        return;
    }
    let Some((text, _, _)) = single_literal(query, "value") else { return };
    if text.trim_start().starts_with(['{', '[']) {
        return;
    }
    let first_word = text.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    if !WRITING_KEYWORDS.contains(&first_word.as_str()) {
        return;
    }
    out.push(Finding::new(
        query.at,
        query.end,
        "query-needs-modifying",
        format!("This query starts with `{first_word}` but the method is not marked @Modifying, so Spring Data executes it as a read. Add @Modifying."),
    ));
}

fn empty_sequence_name(annotation: &Annotation, out: &mut Vec<Finding>) {
    let Some((name, at, end)) = single_literal(annotation, "sequence") else { return };
    if !name.trim().is_empty() {
        return;
    }
    out.push(Finding::new(at, end, "jdbc-empty-sequence-name", "The sequence name of @Sequence is empty, so no value can be fetched. Name the database sequence."));
}

/// A `List` or `Map` of child entities stores the index or key in a column; without `keyColumn` the name comes from the naming strategy.
fn mapped_collection_key_column(annotation: &Annotation, property_type: &TypeRef, out: &mut Vec<Finding>) {
    if !KEYED_COLLECTIONS.contains(&property_type.name.as_str()) || annotation.arg("keyColumn").is_some() {
        return;
    }
    out.push(
        Finding::new(
            annotation.at,
            annotation.end,
            "mapped-collection-key-column",
            format!("@MappedCollection on a {} stores each element's {} in a key column whose name is derived by the naming strategy. Set keyColumn to name it.", property_type.name, if property_type.name == "List" { "index" } else { "key" }),
        )
        .with_severity(Severity::Hint),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use spring_text::java::parse;

    const ALL: Groups = Groups { pageable: true, modifying: true, jdbc_mapping: true };

    fn run_with(text: &str, groups: Groups) -> Vec<(&'static str, String)> {
        let file = parse(text);
        let repositories: Vec<usize> =
            file.types.iter().enumerate().filter(|(_, t)| t.kind == "interface").map(|(i, _)| i).collect();
        check(&file, &repositories, groups).iter().map(|f| (f.code, text[f.start..f.end].to_owned())).collect()
    }

    fn run(text: &str) -> Vec<(&'static str, String)> {
        run_with(text, ALL)
    }

    const PAGE: &str = "import org.springframework.data.domain.Page;\nimport org.springframework.data.domain.Pageable;\ninterface R extends JpaRepository<User, Long> {\n    Page<User> findByName(String name);\n}\n";

    #[test]
    fn page_without_pageable_warns() {
        assert_eq!(run(PAGE), [("page-without-pageable", "findByName".to_owned())]);
        let query = PAGE.replace("Page<User> findByName(String name);", "@Query(\"select u from User u\")\n    Page<User> users();");
        assert_eq!(run(&query), [("page-without-pageable", "users".to_owned())]);
    }

    #[test]
    fn slice_without_pageable_warns() {
        let text = PAGE.replace("Page<User>", "Slice<User>").replace("domain.Page;", "domain.Slice;");
        assert_eq!(run(&text), [("page-without-pageable", "findByName".to_owned())]);
    }

    #[test]
    fn page_with_pageable_is_quiet() {
        assert!(run(&PAGE.replace("String name", "String name, Pageable pageable")).is_empty());
        assert!(run(&PAGE.replace("String name", "String name, PageRequest request")).is_empty());
        assert!(run(&PAGE.replace("String name", "String name, org.acme.CustomPageable p")).is_empty());
    }

    #[test]
    fn page_without_pageable_is_quiet_for_non_query_methods() {
        assert!(run(&PAGE.replace("findByName(String name)", "search(Criteria criteria)")).is_empty());
        assert!(run(&PAGE.replace("findByName(String name)", "findAll()")).is_empty());
    }

    #[test]
    fn page_check_is_quiet_for_library_or_dynamic_name() {
        assert!(run(&PAGE.replace("org.springframework.data.domain.Page;", "com.acme.Page;")).is_empty());
        assert!(run(&PAGE.replace("Page<User> findByName(String name);", "default Page<User> findByName(String name) { return null; }")).is_empty());
        assert!(run(&PAGE.replace("Page<User>", "List<User>")).is_empty());
        assert!(run_with(PAGE, Groups { pageable: false, ..ALL }).is_empty());
    }

    const UPDATE: &str = "import org.springframework.data.jpa.repository.Query;\ninterface R extends JpaRepository<User, Long> {\n    @Query(\"update User u set u.active = false\")\n    int deactivate();\n}\n";

    #[test]
    fn update_query_without_modifying_warns() {
        let found = run(UPDATE);
        assert_eq!(found, [("query-needs-modifying", "@Query(\"update User u set u.active = false\")".to_owned())]);
    }

    #[test]
    fn delete_native_query_without_modifying_warns() {
        let text = UPDATE.replace("@Query(\"update User u set u.active = false\")", "@NativeQuery(\"  DELETE from users\")");
        assert_eq!(run(&text).iter().map(|f| f.0).collect::<Vec<_>>(), ["query-needs-modifying"]);
    }

    #[test]
    fn select_query_is_quiet() {
        assert!(run(&UPDATE.replace("update User u set u.active = false", "select u from User u")).is_empty());
    }

    #[test]
    fn modifying_present_is_quiet() {
        assert!(run(&UPDATE.replace("    @Query", "    @Modifying\n    @Query")).is_empty());
        assert!(run(&UPDATE.replace("    @Query", "    @Procedure\n    @Query")).is_empty());
    }

    #[test]
    fn query_modifying_is_quiet_for_library_or_dynamic_name() {
        let with = |query: &str| run(&UPDATE.replace("\"update User u set u.active = false\"", query));
        assert!(with("UPDATE_QUERY").is_empty());
        assert!(with("\"update User u \" + \"set u.active = false\"").is_empty());
        assert!(with("\"update User u \" + SUFFIX").is_empty());
        assert!(with("\"{ $set: { active: false } }\"").is_empty());
        assert!(with("\"[ { $match: {} } ]\"").is_empty());
        assert!(run(&UPDATE.replace("org.springframework.data.jpa.repository.Query", "com.acme.Query")).is_empty());
        assert!(run_with(UPDATE, Groups { modifying: false, ..ALL }).is_empty());
    }

    const SEQUENCE: &str = "import org.springframework.data.relational.core.mapping.Sequence;\nclass Person {\n    @Id\n    @Sequence(sequence = \"\")\n    Long id;\n}\n";

    #[test]
    fn blank_sequence_name_warns() {
        assert_eq!(run(SEQUENCE), [("jdbc-empty-sequence-name", "\"\"".to_owned())]);
        assert_eq!(run(&SEQUENCE.replace("\"\"", "\"  \"")).len(), 1);
    }

    #[test]
    fn named_sequence_is_quiet() {
        assert!(run(&SEQUENCE.replace("\"\"", "\"person_seq\"")).is_empty());
    }

    #[test]
    fn sequence_name_is_quiet_for_library_or_dynamic_name() {
        assert!(run(&SEQUENCE.replace("\"\"", "SEQUENCE_NAME")).is_empty());
        assert!(run(&SEQUENCE.replace("\"\"", "\"\" + NAME")).is_empty());
        assert!(run(&SEQUENCE.replace("(sequence = \"\")", "")).is_empty());
        assert!(run(&SEQUENCE.replace("org.springframework.data.relational.core.mapping.Sequence", "com.acme.Sequence")).is_empty());
        assert!(run_with(SEQUENCE, Groups { jdbc_mapping: false, ..ALL }).is_empty());
    }

    const COLLECTION: &str = "import org.springframework.data.relational.core.mapping.MappedCollection;\nclass Person {\n    @MappedCollection(idColumn = \"PERSON\")\n    List<Item> items;\n}\n";

    #[test]
    fn list_mapped_collection_without_key_column_hints() {
        assert_eq!(run(COLLECTION), [("mapped-collection-key-column", "@MappedCollection(idColumn = \"PERSON\")".to_owned())]);
        let map = COLLECTION.replace("List<Item>", "Map<String, Item>");
        assert_eq!(run(&map).len(), 1);
        let file = parse(COLLECTION);
        let found = check(&file, &[], ALL);
        assert_eq!(found[0].severity, Severity::Hint);
    }

    #[test]
    fn map_with_key_column_is_quiet() {
        let text = COLLECTION.replace("List<Item>", "Map<String, Item>").replace("idColumn = \"PERSON\"", "idColumn = \"PERSON\", keyColumn = \"K\"");
        assert!(run(&text).is_empty());
    }

    #[test]
    fn set_mapped_collection_is_quiet() {
        assert!(run(&COLLECTION.replace("List<Item>", "Set<Item>")).is_empty());
        assert!(run(&COLLECTION.replace("List<Item>", "Item")).is_empty());
    }

    #[test]
    fn mapped_collection_is_quiet_for_library_or_dynamic_name() {
        assert!(run(&COLLECTION.replace("idColumn = \"PERSON\"", "keyColumn = KEY")).is_empty());
        let foreign = COLLECTION.replace("org.springframework.data.relational.core.mapping.MappedCollection", "com.acme.MappedCollection");
        assert!(run(&foreign).is_empty());
    }

    #[test]
    fn groups_can_be_switched_off() {
        let all = [PAGE, UPDATE, SEQUENCE, COLLECTION].join("");
        assert!(run(&all).len() >= 4);
        let off = Groups { pageable: false, modifying: false, jdbc_mapping: false };
        assert!(run_with(&all, off).is_empty());
    }

    #[test]
    fn ranges_use_utf16_columns_with_multibyte_text() {
        let text = "/* é😀 */ import org.springframework.data.domain.Page;\ninterface R extends JpaRepository<User, Long> {\n    Page<User> findByName(String name);\n}\n";
        let file = parse(text);
        let finding = &check(&file, &[0], ALL)[0];
        let d = finding.diagnostic(text);
        assert_eq!((d.range.start.line, d.range.start.character), (2, 15));
        let line = text.lines().nth(2).unwrap();
        assert_eq!(&line[15..25], "findByName");
    }

    #[test]
    fn crlf_files_report_correct_lines() {
        let text = PAGE.replace('\n', "\r\n");
        let file = parse(&text);
        let d = check(&file, &[0], ALL)[0].diagnostic(&text);
        assert_eq!((d.range.start.line, d.range.start.character), (3, 15));
    }

    #[test]
    fn kotlin_repository_is_checked() {
        let text = "import org.springframework.data.domain.Page\n\ninterface R : JpaRepository<User, Long> {\n    fun findByName(name: String): Page<User>\n    @Query(\"delete from User\")\n    fun purge(): Int\n}\n";
        let file = spring_text::parse_source("R.kt", text);
        let found: Vec<_> = check(&file, &[0], ALL).iter().map(|f| f.code).collect();
        assert_eq!(found, ["page-without-pageable", "query-needs-modifying"]);
    }

    #[test]
    fn kotlin_mapped_collection_is_checked() {
        let text = "class Person(@MappedCollection(idColumn = \"P\") val items: List<Item>)\n";
        let file = spring_text::parse_source("P.kt", text);
        assert_eq!(check(&file, &[], ALL).len(), 1);
    }
}
