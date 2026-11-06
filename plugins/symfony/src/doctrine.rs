//! Checks of Doctrine ORM mappings in PHP attributes, `*.orm.xml` and
//! `*.orm.yml|yaml` files. Written from the Doctrine ORM documentation
//! (attributes reference, association mapping, events, upgrade notes).

use std::ops::Range;

use crate::checks::Finding;
use crate::index::{xml_attribute, xml_tags, yaml_keys};
use crate::php_checks::{arguments, named, trimmed};
use crate::php_text::{attributes, imports, mask, matching_close, AttributeUse, Target};

const MAPPING_NAMESPACE: &str = "Doctrine\\ORM\\Mapping";
const LIFECYCLE_ATTRIBUTES: [&str; 8] = ["PrePersist", "PostPersist", "PreUpdate", "PostUpdate", "PreRemove", "PostRemove", "PostLoad", "PreFlush"];
const NULLABLE_MESSAGE: &str = "A join column is nullable unless you set `nullable: false`; `nullable: true` is already the default.";

pub fn check(path: &str, language: &str, text: &str) -> Vec<Finding> {
    let mut found = match language {
        "php" => php(text),
        "xml" if path.ends_with(".orm.xml") => xml(text),
        "yaml" if path.ends_with(".orm.yml") || path.ends_with(".orm.yaml") => yaml(text),
        _ => Vec::new(),
    };
    found.sort_by_key(|finding| finding.start);
    found
}

fn mapping_name(attribute: &AttributeUse) -> Option<&str> {
    attribute.name.strip_prefix(MAPPING_NAMESPACE)?.strip_prefix('\\')
}

/// Where the `$name` of the attribute's property is written: equal for every attribute of one property.
fn property_at(masked: &str, attribute: &AttributeUse) -> Option<usize> {
    let Target::Property { name } = &attribute.target else { return None };
    masked[attribute.range.end..].find(&format!("${name}")).map(|at| attribute.range.end + at)
}

fn php(text: &str) -> Vec<Finding> {
    let imported = imports(text).iter().any(|import| import.fqcn == MAPPING_NAMESPACE || import.fqcn.starts_with(&format!("{MAPPING_NAMESPACE}\\")));
    if !imported {
        return Vec::new();
    }

    let masked = mask(text);
    let attributes = attributes(text);
    let mut found = Vec::new();

    for attribute in &attributes {
        match mapping_name(attribute) {
            Some("Table") => table(&masked, text, attribute, &mut found),
            Some("OneToMany") => one_to_many(&masked, &attributes, attribute, &mut found),
            Some("OneToOne") => inverse_side_join_column(&masked, &attributes, attribute, &mut found),
            Some("JoinColumn") => nullable_attribute(&masked, attribute, &mut found),
            _ => {}
        }
    }

    lifecycle_callbacks(&masked, &attributes, &mut found);
    found
}

fn table(masked: &str, text: &str, attribute: &AttributeUse, found: &mut Vec<Finding>) {
    if !matches!(attribute.target, Target::Class { .. }) {
        return;
    }

    let arguments = match masked[attribute.range.clone()].contains('(') {
        true => arguments(masked, attribute.arguments.start - 1),
        false => Vec::new(),
    };

    let only_empty_name = arguments.len() == 1 && {
        let argument = trimmed(masked, arguments[0].clone());
        let compact: String = text[argument].chars().filter(|c| !c.is_whitespace()).collect();
        compact == "name:''" || compact == "name:\"\""
    };
    if arguments.is_empty() || only_empty_name {
        found.push(Finding::hint(attribute.range.clone(), "orm-empty-table", "`#[Table]` without a table name does nothing: the table is named after the class by default. Remove it or give it a `name`.".into()));
        return;
    }

    for argument in arguments {
        let Some((name, _)) = named(masked, argument) else { continue };
        let property = &text[name.clone()];
        if property == "indexes" || property == "uniqueConstraints" {
            let replacement = if property == "indexes" { "Index" } else { "UniqueConstraint" };
            found.push(Finding::warn(
                name,
                "orm-table-deprecated-property",
                format!("`{property}` of `#[Table]` is deprecated since ORM 3.2 and has no effect. Declare `#[{replacement}]` attributes on the class instead."),
            ));
        }
    }
}

fn has_named(masked: &str, attribute: &AttributeUse, wanted: &str) -> bool {
    let open = attribute.arguments.start.saturating_sub(1);
    let list = arguments(masked, open);
    list.into_iter().any(|argument| named(masked, argument).is_some_and(|(name, _)| &masked[name] == wanted))
}

/// `mappedBy` written by name or as the second positional argument.
fn has_mapped_by(masked: &str, attribute: &AttributeUse) -> bool {
    if !masked[attribute.range.clone()].contains('(') {
        return false;
    }
    if has_named(masked, attribute, "mappedBy") {
        return true;
    }

    let list = arguments(masked, attribute.arguments.start - 1);
    list.into_iter().skip(1).any(|argument| named(masked, argument).is_none())
}

fn same_property<'a>(masked: &'a str, attributes: &'a [AttributeUse], attribute: &AttributeUse) -> impl Iterator<Item = &'a AttributeUse> {
    let at = property_at(masked, attribute);
    attributes.iter().filter(move |other| at.is_some() && property_at(masked, other) == at)
}

fn one_to_many(masked: &str, attributes: &[AttributeUse], attribute: &AttributeUse, found: &mut Vec<Finding>) {
    if !matches!(attribute.target, Target::Property { .. }) || has_mapped_by(masked, attribute) {
        return;
    }

    let through_join_table = same_property(masked, attributes, attribute).any(|other| mapping_name(other) == Some("JoinTable"));
    if through_join_table {
        return;
    }

    found.push(Finding::warn(
        attribute.range.clone(),
        "orm-one-to-many-mapped-by",
        "A bidirectional one-to-many needs `mappedBy` naming the many-to-one property on the other entity. Without it, map the association through a `#[JoinTable]`.".into(),
    ));
}

fn inverse_side_join_column(masked: &str, attributes: &[AttributeUse], attribute: &AttributeUse, found: &mut Vec<Finding>) {
    if !matches!(attribute.target, Target::Property { .. }) || !has_mapped_by(masked, attribute) {
        return;
    }

    for other in same_property(masked, attributes, attribute).filter(|other| mapping_name(other) == Some("JoinColumn")) {
        found.push(Finding::hint(
            other.range.clone(),
            "orm-inverse-side-join-column",
            "This is the inverse side of the one-to-one (`mappedBy`): the owning side holds the foreign key, so a `#[JoinColumn]` here has no use.".into(),
        ));
    }
}

fn nullable_attribute(masked: &str, attribute: &AttributeUse, found: &mut Vec<Finding>) {
    if !masked[attribute.range.clone()].contains('(') {
        return;
    }

    for argument in arguments(masked, attribute.arguments.start - 1) {
        let Some((name, value)) = named(masked, argument) else { continue };
        if &masked[name.clone()] == "nullable" && &masked[value.clone()] == "true" {
            found.push(Finding::hint(name.start..value.end, "orm-join-column-nullable", NULLABLE_MESSAGE.into()));
        }
    }
}

fn lifecycle_callbacks(masked: &str, attributes: &[AttributeUse], found: &mut Vec<Finding>) {
    for entity in attributes.iter().filter(|a| matches!(mapping_name(a), Some("Entity" | "MappedSuperclass"))) {
        let Target::Class { name } = &entity.target else { continue };

        let marked = attributes.iter().any(|a| mapping_name(a) == Some("HasLifecycleCallbacks") && a.target == entity.target);
        if marked {
            continue;
        }

        let Some(body) = class_body(masked, entity.range.end, name) else { continue };
        for callback in attributes.iter().filter(|a| body.contains(&a.range.start) && matches!(mapping_name(a), Some(n) if LIFECYCLE_ATTRIBUTES.contains(&n))) {
            found.push(Finding::warn(
                callback.range.clone(),
                "orm-lifecycle-callbacks-missing",
                "Doctrine ignores lifecycle callbacks unless the class carries `#[HasLifecycleCallbacks]`.".into(),
            ));
        }
    }
}

fn class_body(masked: &str, from: usize, name: &str) -> Option<Range<usize>> {
    let declaration = masked[from..].find(&format!("class {name}"))? + from;
    let open = masked[declaration..].find('{')? + declaration;
    Some(open..matching_close(masked, open)?)
}

fn xml(text: &str) -> Vec<Finding> {
    let masked = mask_xml_comments(text);
    let mut found = Vec::new();

    for (tag, at) in xml_tags(&masked, "join-column") {
        let Some((value, offset)) = xml_attribute(tag, "nullable") else { continue };
        if value == "true" {
            let start = at + offset - "nullable=\"".len();
            found.push(Finding::hint(start..at + offset + value.len() + 1, "orm-join-column-nullable", NULLABLE_MESSAGE.into()));
        }
    }

    found
}

fn mask_xml_comments(text: &str) -> String {
    let mut out = text.to_owned();
    let mut from = 0;

    while let Some(start) = text[from..].find("<!--").map(|at| from + at) {
        let end = text[start..].find("-->").map_or(text.len(), |at| start + at + 3);
        let spaces: String = text[start..end].bytes().map(|byte| if byte == b'\n' || byte == b'\r' { byte as char } else { ' ' }).collect();
        out.replace_range(start..end, &spaces);
        from = end;
    }

    out
}

fn yaml(text: &str) -> Vec<Finding> {
    yaml_keys(text)
        .into_iter()
        .filter(|entry| entry.key == "nullable" && entry.value == "true")
        .filter(|entry| entry.parents.last().is_some_and(|parent| parent == "joinColumn") || entry.parents.iter().rev().nth(1).is_some_and(|parent| parent == "joinColumns"))
        .map(|entry| Finding::hint(entry.offset..entry.offset + entry.key.len(), "orm-join-column-nullable", NULLABLE_MESSAGE.into()))
        .collect()
}

#[cfg(test)]
mod tests {
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::{Diagnostic, Event, Plugin, Position, Range as TextRange};

    use super::*;
    use crate::index::Index;
    use crate::Main;

    const PATH: &str = "/project/src/Entity/User.php";
    const ORM: &str = "use Doctrine\\ORM\\Mapping as ORM;\n";

    fn entity(body: &str) -> String {
        format!("<?php\nnamespace App\\Entity;\n\n{ORM}\n#[ORM\\Entity]\nclass User\n{{\n{body}\n}}\n")
    }

    fn codes(path: &str, language: &str, text: &str) -> Vec<&'static str> {
        check(path, language, text).iter().map(|finding| finding.code).collect()
    }

    fn php(text: &str) -> Vec<&'static str> {
        codes(PATH, "php", text)
    }

    fn slice<'a>(text: &'a str, code: &str) -> &'a str {
        let found = check(PATH, "php", text).into_iter().find(|finding| finding.code == code).unwrap();
        &text[found.start..found.end]
    }

    #[test]
    fn orm_empty_table_is_a_hint() {
        for attribute in ["#[ORM\\Table]", "#[ORM\\Table()]", "#[ORM\\Table(name: '')]"] {
            let text = format!("<?php\n{ORM}\n{attribute}\n#[ORM\\Entity]\nclass User {{}}\n");
            let found = check(PATH, "php", &text);
            assert_eq!(found.len(), 1, "{attribute}");
            assert_eq!(found[0].code, "orm-empty-table");
            assert_eq!(found[0].severity, openidea_plugin::Severity::Hint);
        }
    }

    #[test]
    fn orm_empty_table_is_quiet_with_a_name() {
        for attribute in ["#[ORM\\Table(name: 'users')]", "#[ORM\\Table('users')]", "#[ORM\\Table(schema: 's')]", "#[ORM\\Table(name: $name)]"] {
            let text = format!("<?php\n{ORM}\n{attribute}\n#[ORM\\Entity]\nclass User {{}}\n");
            assert!(php(&text).is_empty(), "{attribute}");
        }
    }

    #[test]
    fn table_indexes_property_warns() {
        let text = format!("<?php\n{ORM}\n#[ORM\\Table(name: 'u', indexes: [new ORM\\Index(columns: ['a'])], uniqueConstraints: [])]\n#[ORM\\Entity]\nclass User {{}}\n");
        let found = check(PATH, "php", &text);
        let names: Vec<_> = found.iter().map(|f| (f.code, &text[f.start..f.end])).collect();
        assert_eq!(names, [("orm-table-deprecated-property", "indexes"), ("orm-table-deprecated-property", "uniqueConstraints")]);
        assert_eq!(found[0].severity, openidea_plugin::Severity::Warning);
        assert!(found[0].message.contains("3.2"));
    }

    #[test]
    fn table_indexes_property_is_quiet_for_valid_mapping() {
        let text = format!("<?php\n{ORM}\n#[ORM\\Table(name: 'u')]\n#[ORM\\Index(columns: ['a'])]\n#[ORM\\UniqueConstraint(columns: ['b'])]\n#[ORM\\Entity]\nclass User {{}}\n");
        assert!(php(&text).is_empty());
        let text = format!("<?php\n{ORM}\n#[ORM\\Table(name: 'u', options: ['indexes' => 1])]\n#[ORM\\Entity]\nclass User {{}}\n");
        assert!(php(&text).is_empty());
    }

    #[test]
    fn lifecycle_callbacks_missing_warns_once_per_attribute() {
        let text = entity("    #[ORM\\PrePersist]\n    public function a(): void {}\n\n    #[ORM\\PostLoad, ORM\\PreFlush]\n    public function b(): void {}");
        let found = check(PATH, "php", &text);
        let shown: Vec<_> = found.iter().map(|f| (f.code, &text[f.start..f.end])).collect();
        assert_eq!(
            shown,
            [("orm-lifecycle-callbacks-missing", "ORM\\PrePersist"), ("orm-lifecycle-callbacks-missing", "ORM\\PostLoad"), ("orm-lifecycle-callbacks-missing", "ORM\\PreFlush")]
        );
    }

    #[test]
    fn lifecycle_callbacks_missing_is_quiet_with_has_lifecycle_callbacks() {
        let text = format!("<?php\n{ORM}\n#[ORM\\Entity, ORM\\HasLifecycleCallbacks]\nclass User\n{{\n    #[ORM\\PrePersist]\n    public function a(): void {{}}\n}}\n");
        assert!(php(&text).is_empty());
    }

    #[test]
    fn lifecycle_callbacks_missing_is_quiet_outside_entities() {
        let text = format!("<?php\n{ORM}\nclass Listener\n{{\n    #[ORM\\PrePersist]\n    public function a(): void {{}}\n}}\n");
        assert!(php(&text).is_empty());
        let text = format!(
            "<?php\n{ORM}\n#[ORM\\Entity]\nclass A\n{{\n}}\n\n#[ORM\\Entity, ORM\\HasLifecycleCallbacks]\nclass B\n{{\n    #[ORM\\PrePersist]\n    public function a(): void {{}}\n}}\n"
        );
        assert!(php(&text).is_empty());
    }

    #[test]
    fn one_to_many_without_mapped_by_warns() {
        let text = entity("    #[ORM\\OneToMany(targetEntity: Phone::class)]\n    private $phones;");
        assert_eq!(php(&text), ["orm-one-to-many-mapped-by"]);
        assert_eq!(slice(&text, "orm-one-to-many-mapped-by"), "ORM\\OneToMany(targetEntity: Phone::class)");
    }

    #[test]
    fn one_to_many_with_mapped_by_is_quiet() {
        for body in [
            "#[ORM\\OneToMany(targetEntity: Phone::class, mappedBy: 'user')]\n    private $phones;",
            "#[ORM\\OneToMany(Phone::class, 'user')]\n    private $phones;",
            "#[ORM\\OneToMany(targetEntity: Phone::class)]\n    #[ORM\\JoinTable(name: 'users_phones')]\n    private $phones;",
            "#[ORM\\JoinTable(name: 'users_phones')]\n    #[ORM\\OneToMany(targetEntity: Phone::class)]\n    private $phones;",
        ] {
            assert!(php(&entity(&format!("    {body}"))).is_empty(), "{body}");
        }
    }

    #[test]
    fn one_to_many_join_table_belongs_to_its_own_property() {
        let text = entity("    #[ORM\\JoinTable(name: 't')]\n    #[ORM\\ManyToMany(targetEntity: A::class)]\n    private $a;\n\n    #[ORM\\OneToMany(targetEntity: B::class)]\n    private $b;");
        assert_eq!(php(&text), ["orm-one-to-many-mapped-by"]);
    }

    #[test]
    fn inverse_side_join_column_is_a_hint() {
        let text = entity("    #[ORM\\OneToOne(targetEntity: Cart::class, mappedBy: 'user')]\n    #[ORM\\JoinColumn(name: 'cart_id', nullable: false)]\n    private $cart;");
        let found = check(PATH, "php", &text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code, "orm-inverse-side-join-column");
        assert_eq!(found[0].severity, openidea_plugin::Severity::Hint);
        assert_eq!(&text[found[0].start..found[0].end], "ORM\\JoinColumn(name: 'cart_id', nullable: false)");
    }

    #[test]
    fn inverse_side_join_column_is_quiet_for_valid_mapping() {
        for body in [
            "#[ORM\\OneToOne(targetEntity: Cart::class, inversedBy: 'user')]\n    #[ORM\\JoinColumn(name: 'cart_id', nullable: false)]\n    private $cart;",
            "#[ORM\\OneToOne(targetEntity: Cart::class, mappedBy: 'user')]\n    private $cart;",
            "#[ORM\\OneToOne(targetEntity: Cart::class, mappedBy: 'user')]\n    private $cart;\n\n    #[ORM\\ManyToOne]\n    #[ORM\\JoinColumn(nullable: false)]\n    private $other;",
        ] {
            assert!(php(&entity(&format!("    {body}"))).is_empty(), "{body}");
        }
    }

    #[test]
    fn join_column_nullable_true_is_a_hint_in_php_xml_and_yaml() {
        let text = entity("    #[ORM\\ManyToOne]\n    #[ORM\\JoinColumn(name: 'a_id', nullable: true)]\n    private $a;");
        let found = check(PATH, "php", &text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code, "orm-join-column-nullable");
        assert_eq!(found[0].severity, openidea_plugin::Severity::Hint);
        assert_eq!(&text[found[0].start..found[0].end], "nullable: true");

        let xml = "<doctrine-mapping>\n  <entity name=\"User\">\n    <many-to-one field=\"a\">\n      <join-column name=\"a_id\" nullable=\"true\"/>\n    </many-to-one>\n  </entity>\n</doctrine-mapping>\n";
        let found = check("/project/config/doctrine/User.orm.xml", "xml", xml);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code, "orm-join-column-nullable");
        assert!(xml[found[0].start..found[0].end].starts_with("nullable"));

        let yaml = "App\\Entity\\User:\n  type: entity\n  manyToOne:\n    a:\n      joinColumn:\n        name: a_id\n        nullable: true\n";
        for path in ["/project/config/doctrine/User.orm.yml", "/project/config/doctrine/User.orm.yaml"] {
            let found = check(path, "yaml", yaml);
            assert_eq!(found.len(), 1, "{path}");
            assert_eq!(found[0].code, "orm-join-column-nullable");
            assert_eq!(&yaml[found[0].start..found[0].end], "nullable");
        }

        let yaml = "App\\Entity\\User:\n  manyToMany:\n    a:\n      joinTable:\n        joinColumns:\n          user_id:\n            nullable: true\n";
        assert_eq!(codes("/project/User.orm.yml", "yaml", yaml), ["orm-join-column-nullable"]);
    }

    #[test]
    fn join_column_nullable_false_is_quiet() {
        for body in [
            "#[ORM\\JoinColumn(nullable: false)]",
            "#[ORM\\JoinColumn(name: 'a_id')]",
            "#[ORM\\JoinColumn(nullable: $flag)]",
            "#[ORM\\Column(nullable: true)]",
        ] {
            assert!(php(&entity(&format!("    #[ORM\\ManyToOne]\n    {body}\n    private $a;"))).is_empty(), "{body}");
        }

        let xml = "<doctrine-mapping>\n<!-- <join-column nullable=\"true\"/> -->\n<join-column name=\"a\" nullable=\"false\"/>\n<field name=\"x\" nullable=\"true\"/>\n</doctrine-mapping>\n";
        assert!(codes("/project/User.orm.xml", "xml", xml).is_empty());

        let yaml = "App\\Entity\\User:\n  fields:\n    name:\n      nullable: true\n  manyToOne:\n    a:\n      joinColumn:\n        nullable: false\n";
        assert!(codes("/project/User.orm.yml", "yaml", yaml).is_empty());
        assert!(codes("/project/services.yaml", "yaml", "joinColumn:\n  nullable: true\n").is_empty());
        assert!(codes("/project/other.xml", "xml", "<join-column nullable=\"true\"/>").is_empty());
    }

    #[test]
    fn doctrine_alias_import_is_followed() {
        let text = "<?php\nuse Doctrine\\ORM\\Mapping as Orm;\n\n#[Orm\\Table]\nclass User {}\n";
        assert_eq!(php(text), ["orm-empty-table"]);
        let text = "<?php\nuse Doctrine\\ORM\\Mapping\\Table;\n\n#[Table]\nclass User {}\n";
        assert_eq!(php(text), ["orm-empty-table"]);
        let text = "<?php\nuse Doctrine\\ORM\\Mapping\\{Entity, Table};\n\n#[Table()]\nclass User {}\n";
        assert_eq!(php(text), ["orm-empty-table"]);
    }

    #[test]
    fn doctrine_checks_are_quiet_without_the_orm_mapping_import() {
        let text = "<?php\nuse App\\Attribute\\Table;\n\n#[Table]\nclass User\n{\n    #[Table\\OneToMany]\n    private $a;\n}\n";
        assert!(php(text).is_empty());
        let text = "<?php\nnamespace App;\n\n#[Table]\n#[OneToMany]\nclass Table {}\n";
        assert!(php(text).is_empty());
        let text = "<?php\nuse Doctrine\\ORM\\Mapping as ORM;\nuse App\\Table;\n\n#[Table]\nclass User {}\n";
        assert!(php(text).is_empty());
    }

    #[test]
    fn doctrine_checks_ignore_docblock_annotations_and_comments() {
        let text = format!("<?php\n{ORM}\n/**\n * @ORM\\Table(indexes={{}})\n */\n// #[ORM\\Table]\n/* #[ORM\\OneToMany] */\nclass User\n{{\n    /** @ORM\\OneToMany(targetEntity=\"A\") */\n    private $a;\n}}\n");
        assert!(php(&text).is_empty());
    }

    #[test]
    fn doctrine_files_of_other_kinds_are_ignored() {
        assert!(codes("/project/src/User.twig", "twig", "#[ORM\\Table]").is_empty());
        assert!(codes("/project/src/a.json", "json", "<join-column nullable=\"true\"/>").is_empty());
    }

    fn index() -> Index {
        Index::default()
    }

    const TABLE: &str = "<?php\nuse Doctrine\\ORM\\Mapping as ORM;\n\n#[ORM\\Table]\n#[ORM\\Entity]\nclass User {}\n";

    fn run(path: &str, language: &str, text: &str) -> Vec<Diagnostic> {
        let read = |_: &str| None;
        crate::extra_problems(&index(), crate::Groups::all_on(), &read, path, language, text)
    }

    #[test]
    fn doctrine_group_ranges_use_utf16_columns_with_multibyte_text() {
        let text = "<?php\nuse Doctrine\\ORM\\Mapping as ORM;\n\n/* é😀 */ #[ORM\\Table]\n#[ORM\\Entity]\nclass User {}\n";
        let found = run(PATH, "php", text);
        assert_eq!(found[0].range.start, Position::new(3, 12));
    }

    #[test]
    fn doctrine_group_crlf_files_report_correct_lines() {
        let found = run(PATH, "php", &TABLE.replace('\n', "\r\n"));
        assert_eq!(found[0].range, TextRange::new(Position::new(3, 2), Position::new(3, 11)));
    }

    fn published(host: &MockHost, plugin: &mut Main, event: Event) -> Vec<String> {
        plugin.handle_events(&[event], &host.invocation());
        host.diagnostics().get(PATH).map(|found| found.iter().filter_map(|d| d.code.clone()).collect()).unwrap_or_default()
    }

    fn project(host: &MockHost) -> Main {
        host.add_file("composer.json", r#"{"require": {"symfony/framework-bundle": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#);
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();
        plugin
    }

    #[test]
    fn doctrine_group_fixed_file_publishes_empty_list() {
        let host = MockHost::install();
        let mut plugin = project(&host);

        host.add_file("src/Entity/User.php", TABLE);
        assert_eq!(published(&host, &mut plugin, Event::FileOpened(PATH.into())), ["orm-empty-table"]);

        host.add_file("src/Entity/User.php", &TABLE.replace("#[ORM\\Table]", "#[ORM\\Table(name: 'users')]"));
        assert!(published(&host, &mut plugin, Event::FileSaved(PATH.into())).is_empty());
    }

    #[test]
    fn doctrine_group_can_be_switched_off() {
        let host = MockHost::install();
        let mut plugin = project(&host);
        host.add_file("src/Entity/User.php", TABLE);

        assert_eq!(published(&host, &mut plugin, Event::FileOpened(PATH.into())), ["orm-empty-table"]);

        host.set_setting("doctrineChecks", "false");
        assert!(published(&host, &mut plugin, Event::FileOpened(PATH.into())).is_empty());
    }
}
