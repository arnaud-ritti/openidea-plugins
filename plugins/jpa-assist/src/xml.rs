//! `persistence.xml` and JPA mapping files (`orm.xml`): values the schema rules out, and classes that are not in the project.

use openidea_plugin::Severity;
use openidea_plugin::providers::{CompletionItem, CompletionKind};

use crate::checks::Finding;

pub struct Attribute {
    pub name: String,
    pub value: String,
    pub start: usize,
}

pub struct Element {
    pub name: String,
    pub parent: Option<String>,
    pub attributes: Vec<Attribute>,
    pub start: usize,
    pub name_end: usize,
    /// Trimmed text of an element without children, and where it starts.
    pub text: Option<(usize, String)>,
}

impl Element {
    fn attribute(&self, name: &str) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.name == name)
    }
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn skip_to(text: &str, from: usize, end: &str) -> usize {
    text[from..].find(end).map_or(text.len(), |p| from + p + end.len())
}

/// A tolerant reader: tags, attributes and the text of leaf elements. Comments, CDATA, processing instructions and doctypes are skipped.
pub fn parse(text: &str) -> Vec<Element> {
    let b = text.as_bytes();
    let mut elements: Vec<Element> = Vec::new();
    let mut open: Vec<(usize, bool, usize)> = Vec::new();
    let mut i = 0;

    while let Some(p) = text[i..].find('<').map(|p| p + i) {
        let rest = &text[p..];
        if rest.starts_with("<!--") {
            i = skip_to(text, p, "-->");
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            i = skip_to(text, p, "]]>");
            continue;
        }
        if rest.starts_with("<?") {
            i = skip_to(text, p, "?>");
            continue;
        }
        if rest.starts_with("<!") {
            i = skip_to(text, p, ">");
            continue;
        }

        if rest.starts_with("</") {
            let end = skip_to(text, p, ">");
            if let Some((index, has_children, content_start)) = open.pop()
                && !has_children
            {
                let raw = &text[content_start..p];
                let lead = raw.len() - raw.trim_start().len();
                elements[index].text = Some((content_start + lead, raw.trim().to_owned()));
            }
            i = end;
            continue;
        }

        let name_len = rest[1..].bytes().take_while(|&c| !c.is_ascii_whitespace() && c != b'/' && c != b'>').count();
        let name = local(&rest[1..1 + name_len]).to_owned();
        let mut j = p + 1 + name_len;
        let mut attributes = Vec::new();

        loop {
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= b.len() || b[j] == b'>' || b[j] == b'/' {
                break;
            }

            let attribute_len = text[j..].bytes().take_while(|&c| !c.is_ascii_whitespace() && c != b'=' && c != b'>' && c != b'/').count();
            let attribute_name = text[j..j + attribute_len].to_owned();
            j += attribute_len;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if b.get(j) != Some(&b'=') {
                attributes.push(Attribute { name: attribute_name, value: String::new(), start: j });
                continue;
            }

            j += 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            let Some(&quote) = b.get(j).filter(|&&q| q == b'"' || q == b'\'') else { break };
            let value_len = text[j + 1..].bytes().take_while(|&c| c != quote).count();
            attributes.push(Attribute { name: attribute_name, value: text[j + 1..j + 1 + value_len].to_owned(), start: j + 1 });
            j += value_len + 2;
        }

        let close = skip_to(text, j.min(text.len()), ">");
        let self_closing = text[..close].ends_with("/>");

        if let Some(parent) = open.last_mut() {
            parent.1 = true;
        }

        elements.push(Element {
            name,
            parent: open.last().map(|&(index, ..)| elements[index].name.clone()),
            attributes,
            start: p,
            name_end: p + 1 + name_len,
            text: None,
        });

        if !self_closing {
            open.push((elements.len() - 1, false, close));
        }
        i = close;
    }

    elements
}

/// What the project says about a class named in the XML.
pub enum Lookup {
    NotInProject,
    Found { annotated: bool },
}

const TRANSACTION_TYPES: &[&str] = &["JTA", "RESOURCE_LOCAL"];
const SHARED_CACHE_MODES: &[&str] = &["ALL", "NONE", "ENABLE_SELECTIVE", "DISABLE_SELECTIVE", "UNSPECIFIED"];
const VALIDATION_MODES: &[&str] = &["AUTO", "CALLBACK", "NONE"];
const ACCESS_TYPES: &[&str] = &["PROPERTY", "FIELD"];

fn warning(start: usize, end: usize, code: &'static str, message: String) -> Finding {
    Finding::new(start, end, Severity::Warning, code, message)
}

fn class_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|part| {
            part.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$') && part.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        })
}

fn is_mapping_file(elements: &[Element]) -> bool {
    elements.iter().any(|e| e.name == "entity-mappings")
}

/// `lookup`: the class with that fully qualified name in the project. `orm_present`: the project has mapping files that can make any class persistent.
pub fn inspect(text: &str, orm_present: bool, lookup: &dyn Fn(&str) -> Lookup) -> Vec<Finding> {
    let elements = parse(text);
    let mut findings = Vec::new();
    let mut units: Vec<&str> = Vec::new();
    let mut listed: Vec<&str> = Vec::new();
    let mapping = is_mapping_file(&elements);
    let package = elements.iter().find(|e| e.name == "package" && e.parent.as_deref() == Some("entity-mappings")).and_then(|e| e.text.as_ref()).map_or("", |(_, t)| t.as_str());

    let expected = |attribute: Option<&Attribute>, allowed: &[&str], what: &str| -> Option<(usize, usize, String)> {
        let a = attribute?;
        if a.value.contains("${") || allowed.contains(&a.value.as_str()) {
            return None;
        }
        Some((a.start, a.start + a.value.len(), format!("'{}' is not a valid {what}: use {}.", a.value, allowed.join(", "))))
    };

    for e in &elements {
        let parent = e.parent.as_deref();

        let checked = match (e.name.as_str(), parent) {
            ("persistence-unit", Some("persistence")) => expected(e.attribute("transaction-type"), TRANSACTION_TYPES, "transaction type"),
            ("entity" | "mapped-superclass" | "embeddable", Some("entity-mappings")) => expected(e.attribute("access"), ACCESS_TYPES, "access type"),
            ("shared-cache-mode", _) => e.text.as_ref().and_then(|(at, t)| {
                (!t.contains("${") && !SHARED_CACHE_MODES.contains(&t.as_str())).then(|| (*at, at + t.len(), format!("'{t}' is not a valid shared cache mode: use {}.", SHARED_CACHE_MODES.join(", ")))
            )}),
            ("validation-mode", _) => e.text.as_ref().and_then(|(at, t)| {
                (!t.contains("${") && !VALIDATION_MODES.contains(&t.as_str())).then(|| (*at, at + t.len(), format!("'{t}' is not a valid validation mode: use {}.", VALIDATION_MODES.join(", ")))
            )}),
            ("exclude-unlisted-classes", _) => e.text.as_ref().and_then(|(at, t)| {
                (!t.is_empty() && !t.contains("${") && !matches!(t.as_str(), "true" | "false")).then(|| (*at, at + t.len(), format!("'{t}' is not true or false."))
            )}),
            _ => None,
        };

        if let Some((start, end, message)) = checked {
            findings.push(warning(start, end, "jpa-xml-value", message));
        }

        match (e.name.as_str(), parent) {
            ("persistence-unit", Some("persistence")) => match e.attribute("name") {
                None => findings.push(warning(e.start, e.name_end, "jpa-xml-unit-name", "A persistence unit needs a name attribute.".into())),
                Some(name) if units.contains(&name.value.as_str()) => {
                    findings.push(warning(name.start, name.start + name.value.len(), "jpa-xml-unit-name", format!("Another persistence unit is named '{}'.", name.value)));
                }
                Some(name) => units.push(&name.value),
            },
            ("property", Some("properties")) if e.attribute("name").is_none_or(|n| n.value.trim().is_empty()) => {
                findings.push(warning(e.start, e.name_end, "jpa-xml-property", "A property needs a name attribute.".into()));
            }
            ("provider", Some("persistence-unit")) => {
                if let Some((at, value)) = e.text.as_ref().filter(|(_, v)| !v.contains("${") && !class_name(v)) {
                    findings.push(warning(*at, at + value.len(), "jpa-xml-provider", format!("'{value}' is not a class name.")));
                }
            }
            ("class", Some("persistence-unit")) => {
                let Some((at, value)) = e.text.as_ref() else {
                    findings.push(warning(e.start, e.name_end, "jpa-xml-class", "This class element names no class.".into()));
                    continue;
                };
                if value.contains("${") {
                    continue;
                }
                if !class_name(value) {
                    findings.push(warning(*at, at + value.len(), "jpa-xml-class", format!("'{value}' is not a class name.")));
                    continue;
                }
                if listed.contains(&value.as_str()) {
                    findings.push(Finding::new(*at, at + value.len(), Severity::Info, "jpa-xml-class-duplicate", format!("{value} is already listed in this file.")));
                }
                listed.push(value);

                match lookup(value) {
                    Lookup::NotInProject => findings.push(Finding::new(
                        *at,
                        at + value.len(),
                        Severity::Info,
                        "jpa-xml-class-unknown",
                        format!("{value} is not a class of this project's sources (it can come from a dependency)."),
                    )),
                    Lookup::Found { annotated: false } if !orm_present && !mapping => findings.push(Finding::new(
                        *at,
                        at + value.len(),
                        Severity::Info,
                        "jpa-xml-class-not-persistent",
                        format!("{value} carries none of @Entity, @MappedSuperclass, @Embeddable or @Converter."),
                    )),
                    Lookup::Found { .. } => {}
                }
            }
            ("entity" | "mapped-superclass" | "embeddable", Some("entity-mappings")) => {
                let Some(class) = e.attribute("class").filter(|c| !c.value.contains("${") && class_name(&c.value)) else { continue };
                let fqcn = if class.value.contains('.') || package.is_empty() { class.value.clone() } else { format!("{package}.{}", class.value) };

                if matches!(lookup(&fqcn), Lookup::NotInProject) {
                    findings.push(Finding::new(
                        class.start,
                        class.start + class.value.len(),
                        Severity::Info,
                        "jpa-xml-class-unknown",
                        format!("{fqcn} is not a class of this project's sources (it can come from a dependency)."),
                    ));
                }
            }
            _ => {}
        }
    }

    findings
}

const PROPERTIES: &[(&str, &str)] = &[
    ("schema-generation.database.action", "none, create, drop-and-create, drop"),
    ("jdbc.driver", "JDBC driver class"),
    ("jdbc.url", "JDBC URL"),
    ("jdbc.user", "database user"),
    ("jdbc.password", "database password"),
    ("lock.timeout", "milliseconds"),
    ("query.timeout", "milliseconds"),
    ("sharedCache.mode", "ALL, NONE, ENABLE_SELECTIVE, DISABLE_SELECTIVE"),
    ("validation.mode", "AUTO, CALLBACK, NONE"),
];

const HIBERNATE_PROPERTIES: &[(&str, &str)] = &[
    ("hibernate.dialect", "SQL dialect class"),
    ("hibernate.hbm2ddl.auto", "none, validate, update, create, create-drop"),
    ("hibernate.show_sql", "true or false"),
    ("hibernate.format_sql", "true or false"),
    ("hibernate.default_schema", "schema name"),
    ("hibernate.default_catalog", "catalog name"),
    ("hibernate.jdbc.batch_size", "statements per batch"),
    ("hibernate.order_inserts", "true or false"),
    ("hibernate.order_updates", "true or false"),
    ("hibernate.generate_statistics", "true or false"),
    ("hibernate.use_sql_comments", "true or false"),
    ("hibernate.cache.use_second_level_cache", "true or false"),
    ("hibernate.cache.use_query_cache", "true or false"),
    ("hibernate.connection.provider_class", "connection provider class"),
];

const PROPERTY_VALUES: &[(&str, &[&str])] = &[
    ("hbm2ddl.auto", &["none", "validate", "update", "create", "create-drop"]),
    ("schema-generation.database.action", &["none", "create", "drop-and-create", "drop"]),
    ("sharedCache.mode", &["ALL", "NONE", "ENABLE_SELECTIVE", "DISABLE_SELECTIVE", "UNSPECIFIED"]),
    ("validation.mode", &["AUTO", "CALLBACK", "NONE"]),
];

/// Names for `name="` and values for `value="` inside the `<property` tag being typed (`tag` is the text from its `<`).
pub fn property_completion(tag: &str, file: &str) -> Vec<CompletionItem> {
    let open_quote = |attribute: &str| -> Option<(usize, &str)> {
        let at = tag.rfind(&format!("{attribute}=\""))? + attribute.len() + 2;
        (!tag[at..].contains('"')).then_some((at, &tag[at..]))
    };

    if let Some((_, typed)) = open_quote("name") {
        let prefix = if file.contains("jakarta.persistence") || !file.contains("javax.persistence") { "jakarta.persistence." } else { "javax.persistence." };
        let jpa = PROPERTIES.iter().map(|(n, d)| (format!("{prefix}{n}"), *d));
        let hibernate = HIBERNATE_PROPERTIES.iter().map(|(n, d)| (n.to_string(), *d));

        return jpa
            .chain(hibernate)
            .filter(|(n, _)| n.starts_with(typed))
            .map(|(n, d)| {
                let mut item = CompletionItem::new(&n, CompletionKind::Property);
                item.detail = Some(d.to_owned());
                item
            })
            .collect();
    }

    let Some((_, typed)) = open_quote("value") else { return Vec::new() };
    let Some(name) = tag.split("name=\"").nth(1).and_then(|r| r.split('"').next()) else { return Vec::new() };

    PROPERTY_VALUES
        .iter()
        .filter(|(suffix, _)| name.ends_with(suffix))
        .flat_map(|(_, values)| values.iter())
        .filter(|v| v.starts_with(typed))
        .map(|v| CompletionItem::new(v, CompletionKind::Value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(text: &str, lookup: &dyn Fn(&str) -> Lookup) -> Vec<(&'static str, String)> {
        inspect(text, false, lookup).iter().map(|f| (f.code, text[f.start..f.end].to_owned())).collect()
    }

    fn found(_: &str) -> Lookup {
        Lookup::Found { annotated: true }
    }

    #[test]
    fn reads_attributes_text_and_skips_comments() {
        let text = "<?xml version=\"1.0\"?>\n<!-- <class>x</class> -->\n<persistence xmlns=\"x\"><persistence-unit name='a' transaction-type=\"JTA\"><class> shop.Order </class><properties/></persistence-unit></persistence>";
        let elements = parse(text);
        let names: Vec<&str> = elements.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["persistence", "persistence-unit", "class", "properties"]);
        assert_eq!(elements[1].attributes[1].value, "JTA");
        let (at, class) = elements[2].text.clone().unwrap();
        assert_eq!((&text[at..at + class.len()], class.as_str()), ("shop.Order", "shop.Order"));
        assert_eq!(elements[3].parent.as_deref(), Some("persistence-unit"));
    }

    #[test]
    fn schema_values_are_checked() {
        let text = "<persistence><persistence-unit name=\"a\" transaction-type=\"XA\"><provider>not a class</provider><shared-cache-mode>MOST</shared-cache-mode><validation-mode>auto</validation-mode><exclude-unlisted-classes>yes</exclude-unlisted-classes><properties><property value=\"1\"/></properties></persistence-unit><persistence-unit name=\"a\"/><persistence-unit/></persistence>";
        let found = codes(text, &found);
        let kinds: Vec<&str> = found.iter().map(|(c, _)| *c).collect();
        assert_eq!(
            kinds,
            ["jpa-xml-value", "jpa-xml-provider", "jpa-xml-value", "jpa-xml-value", "jpa-xml-value", "jpa-xml-property", "jpa-xml-unit-name", "jpa-xml-unit-name"]
        );
        assert_eq!(found[0].1, "XA");
        assert_eq!(found[6].1, "a");
    }

    #[test]
    fn valid_files_and_placeholders_are_quiet() {
        let text = "<persistence><persistence-unit name=\"a\" transaction-type=\"RESOURCE_LOCAL\"><provider>org.hibernate.jpa.HibernatePersistenceProvider</provider><shared-cache-mode>${mode}</shared-cache-mode><validation-mode>NONE</validation-mode><exclude-unlisted-classes/><properties><property name=\"p\" value=\"v\"/></properties></persistence-unit></persistence>";
        assert!(codes(text, &found).is_empty());
    }

    #[test]
    fn classes_are_looked_up_in_the_project() {
        let text = "<persistence><persistence-unit name=\"a\"><class>shop.Order</class><class>vendor.Lib</class><class>shop.Plain</class><class>shop.Order</class><class>bad name</class><class/></persistence-unit></persistence>";
        let lookup = |name: &str| match name {
            "shop.Order" => Lookup::Found { annotated: true },
            "shop.Plain" => Lookup::Found { annotated: false },
            _ => Lookup::NotInProject,
        };
        let kinds: Vec<&str> = codes(text, &lookup).iter().map(|(c, _)| *c).collect();
        assert_eq!(kinds, ["jpa-xml-class-unknown", "jpa-xml-class-not-persistent", "jpa-xml-class-duplicate", "jpa-xml-class", "jpa-xml-class"]);

        let quiet: Vec<_> = inspect(text, true, &lookup).iter().map(|f| f.code).collect();
        assert!(!quiet.contains(&"jpa-xml-class-not-persistent"));
    }

    #[test]
    fn mapping_files_resolve_classes_against_their_package() {
        let text = "<entity-mappings><package>shop</package><entity class=\"Order\" access=\"FIELD\"/><embeddable class=\"vendor.Money\" access=\"METHOD\"/></entity-mappings>";
        let lookup = |name: &str| if name == "shop.Order" { Lookup::Found { annotated: false } } else { Lookup::NotInProject };
        let found = codes(text, &lookup);
        assert_eq!(found, [("jpa-xml-value", "METHOD".to_owned()), ("jpa-xml-class-unknown", "vendor.Money".to_owned())]);
    }
}
