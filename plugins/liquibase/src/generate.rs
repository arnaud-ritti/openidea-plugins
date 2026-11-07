use crate::changelog::{Changelog, Format};

pub fn next_id(log: &Changelog) -> String {
    let highest = log.change_sets.iter().filter_map(|set| set.id.as_deref()?.parse::<u64>().ok()).max();
    (highest.unwrap_or(0) + 1).to_string()
}

fn closing_tag_line(text: &str) -> Option<usize> {
    let tag = text.rfind("</databaseChangeLog>")?;
    Some(text[..tag].rfind('\n').map_or(0, |newline| newline + 1))
}

/// Offset to insert at and the text, or `None` for JSON and for XML without a closing root tag.
pub fn changeset_insertion(format: Format, text: &str, id: &str, author: &str) -> Option<(usize, String)> {
    match format {
        Format::Xml => {
            let at = closing_tag_line(text)?;
            let block = format!("    <changeSet id=\"{id}\" author=\"{author}\">\n        \n    </changeSet>\n");
            Some((at, block))
        }
        Format::Yaml => {
            let separator = if text.ends_with('\n') || text.is_empty() { "" } else { "\n" };
            let block = format!("{separator}  - changeSet:\n      id: {id}\n      author: {author}\n      changes:\n        - \n");
            Some((text.len(), block))
        }
        Format::Sql => {
            let separator = if text.ends_with('\n') || text.is_empty() { "" } else { "\n" };
            Some((text.len(), format!("{separator}\n--changeset {author}:{id}\n")))
        }
        Format::Json => None,
    }
}

pub fn include_insertion(format: Format, text: &str, file: &str) -> Option<(usize, String)> {
    match format {
        Format::Xml => {
            let at = closing_tag_line(text)?;
            Some((at, format!("    <include file=\"{file}\" relativeToChangelogFile=\"true\"/>\n")))
        }
        Format::Yaml => {
            let separator = if text.ends_with('\n') || text.is_empty() { "" } else { "\n" };
            Some((text.len(), format!("{separator}  - include:\n      file: {file}\n      relativeToChangelogFile: true\n")))
        }
        Format::Sql | Format::Json => None,
    }
}

pub fn changelog_source(format: Format, author: &str) -> Option<String> {
    match format {
        Format::Xml => Some(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<databaseChangeLog\n        xmlns=\"http://www.liquibase.org/xml/ns/dbchangelog\"\n        xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"\n        xsi:schemaLocation=\"http://www.liquibase.org/xml/ns/dbchangelog\n                            https://www.liquibase.org/xml/ns/dbchangelog/dbchangelog-latest.xsd\">\n\n    <changeSet id=\"1\" author=\"{author}\">\n        \n    </changeSet>\n</databaseChangeLog>\n"
        )),
        Format::Yaml => Some(format!("databaseChangeLog:\n  - changeSet:\n      id: 1\n      author: {author}\n      changes:\n        - \n")),
        Format::Sql => Some(format!("--liquibase formatted sql\n\n--changeset {author}:1\n")),
        Format::Json => Some(format!(
            "{{\n  \"databaseChangeLog\": [\n    {{\n      \"changeSet\": {{\n        \"id\": \"1\",\n        \"author\": \"{author}\",\n        \"changes\": []\n      }}\n    }}\n  ]\n}}\n"
        )),
    }
}

pub fn extension(format: Format) -> &'static str {
    match format {
        Format::Xml => "xml",
        Format::Yaml => "yaml",
        Format::Json => "json",
        Format::Sql => "sql",
    }
}
