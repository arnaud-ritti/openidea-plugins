use std::collections::BTreeMap;

use openidea_plugin::{Diagnostic, Location, Position, Range, RelatedInformation, Severity};

use crate::naming::{Kind, Name, Naming, parse, version_key};

const RESOURCES: &str = "/src/main/resources/";

fn start_of_file() -> Range {
    Range::new(Position::new(0, 0), Position::new(0, 0))
}

/// The part before `/src/main/resources/`; the project root's own module is `""`.
fn module_of(path: &str, root: &str) -> String {
    match path.split_once(RESOURCES) {
        Some((module, _)) if module != root => module.to_owned(),
        _ => String::new(),
    }
}

/// The directory a `{vendor}` location resolved to: Flyway scans only one, so each is its own group.
fn vendor_of(path: &str, root: &str, vendor_prefixes: &[String]) -> String {
    let after_resources = path.split_once(RESOURCES).map(|(_, rest)| rest);
    for rest in after_resources.into_iter().chain([relative(path, root)]) {
        for prefix in vendor_prefixes {
            let vendor = rest.strip_prefix(prefix.as_str()).and_then(|tail| tail.split_once('/'));
            if let Some((vendor, _)) = vendor {
                return vendor.to_owned();
            }
        }
    }
    String::new()
}

fn relative<'a>(path: &'a str, root: &str) -> &'a str {
    path.strip_prefix(root).map_or(path, |rest| rest.trim_start_matches('/'))
}

/// Every file that is a migration or callback by name (also invalid ones) → its problems (empty: none).
pub fn check(root: &str, files: &[String], naming: &Naming, vendor_prefixes: &[String]) -> BTreeMap<String, Vec<Diagnostic>> {
    let mut results: BTreeMap<String, Vec<Diagnostic>> = BTreeMap::new();
    let mut groups: BTreeMap<(String, String, Kind, Vec<String>), Vec<(&str, String)>> = BTreeMap::new();

    for path in files {
        let file_name = path.rsplit('/').next().unwrap_or(path);

        match parse(file_name, naming) {
            Name::NotMigration => {}
            Name::Callback => {
                results.entry(path.clone()).or_default();
            }
            Name::Invalid(message) => {
                let diagnostic = Diagnostic::new(start_of_file(), Severity::Warning, message).code("migration-name");
                results.entry(path.clone()).or_default().push(diagnostic);
            }
            Name::Migration { kind, version, description } => {
                results.entry(path.clone()).or_default();
                let (key, written) = match kind {
                    Kind::Repeatable => (vec![description.clone()], description),
                    _ => (version_key(&version), version),
                };
                groups.entry((module_of(path, root), vendor_of(path, root, vendor_prefixes), kind, key)).or_default().push((path, written));
            }
        }
    }

    for ((_, _, kind, _), members) in groups.iter().filter(|(_, members)| members.len() > 1) {
        for (path, written) in members {
            let others: Vec<&(&str, String)> = members.iter().filter(|(other, _)| other != path).collect();
            let first = relative(others[0].0, root);
            let (code, message) = match kind {
                Kind::Repeatable => ("duplicate-repeatable", format!("Repeatable migration `{written}` is also defined in {first}.")),
                _ => ("duplicate-version", format!("Version {written} is also used by {first}.")),
            };

            let mut diagnostic = Diagnostic::new(start_of_file(), Severity::Error, message).code(code);
            diagnostic.related = others
                .iter()
                .map(|(other, _)| RelatedInformation {
                    location: Location { path: (*other).to_owned(), range: start_of_file() },
                    message: "Also here".to_owned(),
                })
                .collect();
            results.entry((*path).to_owned()).or_default().push(diagnostic);
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::Severity;

    const DIR: &str = "/project/src/main/resources/db/migration";

    fn files(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| if n.starts_with('/') { (*n).to_owned() } else { format!("{DIR}/{n}") }).collect()
    }

    fn messages(results: &BTreeMap<String, Vec<Diagnostic>>, name: &str) -> Vec<String> {
        results[&format!("{DIR}/{name}")].iter().map(|d| d.message.clone()).collect()
    }

    #[test]
    fn reports_bad_names_as_warnings() {
        let results = check("/project", &files(&["V1__init.sql", "V2_bad.sql", "beforeMigrate.sql", "README.md"]), &Naming::default(), &[]);
        assert_eq!(results.len(), 3, "README.md is not a migration");
        assert!(results[&format!("{DIR}/V1__init.sql")].is_empty());
        assert!(results[&format!("{DIR}/beforeMigrate.sql")].is_empty());
        let bad = &results[&format!("{DIR}/V2_bad.sql")];
        assert_eq!(bad[0].severity, Severity::Warning);
        assert_eq!(bad[0].code.as_deref(), Some("migration-name"));
    }

    #[test]
    fn reports_duplicate_versions_and_repeatables() {
        let results = check(
            "/project",
            &files(&["V1_1__a.sql", "V1.1__b.sql", "U1_1__a.sql", "B1_1__base.sql", "V2__c.sql", "R__views.sql", "/project/sql/R__views.sql"]),
            &Naming::default(),
            &[],
        );
        assert_eq!(messages(&results, "V1_1__a.sql"), ["Version 1_1 is also used by src/main/resources/db/migration/V1.1__b.sql."]);
        assert_eq!(messages(&results, "V1.1__b.sql"), ["Version 1.1 is also used by src/main/resources/db/migration/V1_1__a.sql."]);
        assert!(messages(&results, "U1_1__a.sql").is_empty());
        assert!(messages(&results, "B1_1__base.sql").is_empty());
        assert!(messages(&results, "V2__c.sql").is_empty());
        assert_eq!(messages(&results, "R__views.sql"), ["Repeatable migration `views` is also defined in sql/R__views.sql."]);

        let duplicate = &results[&format!("{DIR}/V1_1__a.sql")][0];
        assert_eq!(duplicate.severity, Severity::Error);
        assert_eq!(duplicate.code.as_deref(), Some("duplicate-version"));
        assert_eq!(duplicate.related[0].location.path, format!("{DIR}/V1.1__b.sql"));
    }

    #[test]
    fn vendor_directories_are_separate_duplicate_groups() {
        let vendors = ["db/migration/".to_owned()];
        let files = files(&["mysql/V1__init.sql", "postgresql/V1__init.sql"]);
        let results = check("/project", &files, &Naming::default(), &vendors);
        assert!(results.values().all(Vec::is_empty), "{results:?}");

        let files = files_in_one_vendor();
        let results = check("/project", &files, &Naming::default(), &vendors);
        assert!(results.values().all(|list| list.len() == 1), "{results:?}");
    }

    fn files_in_one_vendor() -> Vec<String> {
        files(&["mysql/V1__init.sql", "mysql/V1__again.sql"])
    }

    #[test]
    fn duplicates_stay_within_a_module() {
        let results = check(
            "/project",
            &files(&["/project/orders/src/main/resources/db/migration/V1__init.sql", "/project/billing/src/main/resources/db/migration/V1__init.sql"]),
            &Naming::default(),
            &[],
        );
        assert!(results.values().all(Vec::is_empty), "{results:?}");
    }
}
