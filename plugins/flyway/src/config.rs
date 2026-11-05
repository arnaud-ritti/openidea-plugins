//! Flyway project configuration: where migrations live and how they are named.

use crate::naming::Naming;
use openidea_plugin::{Project, host};
use spring_text::config::{canonical, is_config_file, keys, properties};

pub const CONFIG_FILES: &[&str] = &["flyway.toml", "flyway.conf", "conf/flyway.toml", "conf/flyway.conf"];

const DEFAULT_LOCATION: &str = "classpath:db/migration";
const FLYWAY_FILE_LOCATION: &str = "filesystem:sql";

/// `(key, value)` of Flyway settings, keys as written minus a leading `spring.`; list values one pair per item.
pub fn settings(path: &str, text: &str) -> Vec<(String, String)> {
    if path.ends_with(".toml") {
        return toml_settings(text);
    }

    let entries = if path.ends_with(".conf") { properties(text) } else { keys(path, text) };
    let mut pairs = Vec::new();
    for entry in entries {
        let key = entry.key.split('[').next().unwrap_or_default();
        let key = key.strip_prefix("spring.").unwrap_or(key);
        if key.starts_with("flyway.") {
            push_values(&mut pairs, key, &entry.value);
        }
    }
    pairs
}

fn push_values(pairs: &mut Vec<(String, String)>, key: &str, value: &str) {
    for item in value.split(',').map(str::trim).filter(|item| !item.is_empty()) {
        pairs.push((key.to_owned(), item.to_owned()));
    }
}

// ponytail: arrays of up to 6 lines; use the toml crate for longer or nested ones
fn toml_settings(text: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut table = String::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let mut line = strip_comment(line).trim().to_owned();
        if line.split_once('=').is_some_and(|(_, value)| value.trim().starts_with('[') && !value.contains(']')) {
            for next in lines.by_ref().take(5) {
                line.push_str(strip_comment(next).trim());
                if next.contains(']') {
                    break;
                }
            }
        }
        let line = line.as_str();
        if let Some(header) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            table = header.trim().to_owned();
            continue;
        }

        let Some((key, value)) = line.split_once('=') else { continue };
        if table != "flyway" && !table.starts_with("flyway.") {
            continue;
        }

        let full_key = format!("{table}.{}", key.trim());
        let value = value.trim();
        match value.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            Some(items) => {
                for item in items.split(',') {
                    push_values(&mut pairs, &full_key, unquote(item.trim()));
                }
            }
            None => push_values(&mut pairs, &full_key, unquote(value)),
        }
    }
    pairs
}

fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    for (at, c) in line.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(open), _) if open == c => quote = None,
            (None, '#') => return &line[..at],
            _ => {}
        }
    }
    line
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value.strip_prefix(quote).and_then(|rest| rest.strip_suffix(quote)) {
            return inner;
        }
    }
    value
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    pub locations: Vec<String>,
    pub naming: Naming,
    pub placeholders: Vec<String>,
}

fn push_unique(list: &mut Vec<String>, value: &str) {
    if !list.iter().any(|existing| existing == value) {
        list.push(value.to_owned());
    }
}

impl Config {
    pub fn from_settings(pairs: &[(String, String)], flyway_file: bool) -> Config {
        let mut config = Config::default();
        let mut suffixes: Vec<String> = Vec::new();
        for (key, value) in pairs {
            if let Some(name) = key.strip_prefix("flyway.placeholders.") {
                push_unique(&mut config.placeholders, name);
                continue;
            }

            let key = canonical(key);
            let naming = &mut config.naming;
            match key.strip_prefix("flyway.") {
                Some("locations") => push_unique(&mut config.locations, value),
                Some("sqlmigrationprefix") => naming.versioned = value.clone(),
                Some("undosqlmigrationprefix") => naming.undo = value.clone(),
                Some("repeatablesqlmigrationprefix") => naming.repeatable = value.clone(),
                Some("baselinemigrationprefix") => naming.baseline = value.clone(),
                Some("sqlmigrationseparator") => naming.separator = value.clone(),
                Some("sqlmigrationsuffixes") => suffixes.push(value.clone()),
                _ => {}
            }
        }

        if !suffixes.is_empty() {
            config.naming.suffixes = suffixes;
        }

        if config.locations.is_empty() {
            config.locations.push(DEFAULT_LOCATION.to_owned());
            if flyway_file {
                config.locations.push(FLYWAY_FILE_LOCATION.to_owned());
            }
        }
        config
    }
}

pub fn load(project: &Project) -> Config {
    let mut pairs = Vec::new();
    let mut flyway_file = false;
    for file in CONFIG_FILES {
        let Ok(text) = project.read_file(file) else { continue };
        flyway_file = true;
        pairs.extend(settings(file, &text));
        break;
    }

    let spring_files = project.find_files("**/src/main/resources/application*", 100).unwrap_or_default();
    for file in spring_files.iter().filter(|file| is_config_file(file)) {
        if let Ok(text) = project.read_file(file) {
            pairs.extend(settings(file, &text));
        }
    }

    let mut config = Config::from_settings(&pairs, flyway_file);
    if let Some(setting) = host::setting_string("locations") {
        let mut locations = Vec::new();
        for location in setting.split(',').map(str::trim).filter(|location| !location.is_empty()) {
            push_unique(&mut locations, location);
        }
        if !locations.is_empty() {
            config.locations = locations;
        }
    }
    config
}

/// Directories (relative, with a trailing `/`) that precede `{vendor}` in the locations.
pub fn vendor_prefixes(locations: &[String], root: &str) -> Vec<String> {
    let mut prefixes = Vec::new();
    for location in locations {
        let Some((before, _)) = location.split_once("{vendor}") else { continue };
        let path = before.split_once(':').map_or(before, |(_, path)| path);
        let path = path.strip_prefix("./").unwrap_or(path);
        let path = path.strip_prefix(root.trim_end_matches('/')).unwrap_or(path).trim_matches('/');
        prefixes.push(if path.is_empty() { String::new() } else { format!("{path}/") });
    }
    prefixes
}

// ponytail: main resources only; add src/test/resources when test migrations matter
pub fn location_glob(location: &str, root: &str) -> Option<String> {
    let location = location.replace("{vendor}", "*");
    let location = location.trim_end_matches('/');
    let (scheme, path) = match location.split_once(':') {
        Some((scheme, path)) => (scheme, path),
        None => ("classpath", location),
    };

    match scheme {
        "classpath" => Some(format!("**/src/main/resources/{}/**/*", path.trim_matches('/'))),
        "filesystem" => {
            let path = path.strip_prefix("./").unwrap_or(path);
            let relative = match path.starts_with('/') {
                true => path.strip_prefix(root.trim_end_matches('/'))?.strip_prefix('/')?,
                false => path,
            };
            Some(format!("{relative}/**/*"))
        }
        _ => None,
    }
}

pub fn migration_files(project: &Project, config: &Config) -> Vec<String> {
    let Ok(root) = project.root() else { return Vec::new() };
    let mut files = Vec::new();
    for glob in config.locations.iter().filter_map(|location| location_glob(location, &root)) {
        for file in project.find_files(&glob, 10_000).unwrap_or_default() {
            if config.naming.suffixes.iter().any(|suffix| file.ends_with(suffix.as_str())) {
                files.push(file);
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

/// `flyway.toml`, `flyway-dev.conf`, `flyway.user.toml`… (file name starts with `flyway`, ends `.toml`/`.conf`).
pub fn is_flyway_config(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.starts_with("flyway") && (name.ends_with(".toml") || name.ends_with(".conf"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect()
    }

    #[test]
    fn reads_flyway_keys_from_each_format() {
        assert_eq!(
            settings("/project/flyway.conf", "flyway.locations=filesystem:sql, classpath:db\n# comment\nflyway.url=jdbc:h2:mem\n"),
            pairs(&[("flyway.locations", "filesystem:sql"), ("flyway.locations", "classpath:db"), ("flyway.url", "jdbc:h2:mem")])
        );
        let toml = "[environments.dev]\nurl = \"x\"\n[flyway]\nlocations = [\"filesystem:sql\", 'filesystem:more']\nsqlMigrationPrefix = \"M\" # comment\n[flyway.placeholders]\ntenant = \"acme\"\n";
        assert_eq!(
            settings("/project/flyway.toml", toml),
            pairs(&[
                ("flyway.locations", "filesystem:sql"),
                ("flyway.locations", "filesystem:more"),
                ("flyway.sqlMigrationPrefix", "M"),
                ("flyway.placeholders.tenant", "acme"),
            ])
        );
        let yaml = "spring:\n  flyway:\n    locations:\n      - classpath:db/one\n      - classpath:db/two\n    placeholders:\n      tenant: acme\n  datasource:\n    url: x\n";
        assert_eq!(
            settings("/project/src/main/resources/application.yml", yaml),
            pairs(&[("flyway.locations", "classpath:db/one"), ("flyway.locations", "classpath:db/two"), ("flyway.placeholders.tenant", "acme")])
        );
        assert_eq!(
            settings("/project/src/main/resources/application.properties", "spring.flyway.sql-migration-prefix=M\nserver.port=8080\n"),
            pairs(&[("flyway.sql-migration-prefix", "M")])
        );
    }

    #[test]
    fn reads_multi_line_toml_arrays() {
        let toml = "[flyway]\nlocations = [\n  \"filesystem:sql\", # first\n  'filesystem:more',\n]\nsqlMigrationPrefix = \"M\"\n";
        assert_eq!(
            settings("/project/flyway.toml", toml),
            pairs(&[("flyway.locations", "filesystem:sql"), ("flyway.locations", "filesystem:more"), ("flyway.sqlMigrationPrefix", "M")])
        );
    }

    #[test]
    fn vendor_prefixes_precede_the_vendor_segment() {
        let locations = ["classpath:db/migration/{vendor}".to_owned(), "filesystem:/project/sql/{vendor}/".to_owned(), "classpath:db/plain".to_owned()];
        assert_eq!(vendor_prefixes(&locations, "/project"), ["db/migration/", "sql/"]);
    }

    #[test]
    fn config_accepts_flyway_and_spring_spellings() {
        let config = Config::from_settings(
            &pairs(&[
                ("flyway.sqlMigrationPrefix", "M"),
                ("flyway.sql-migration-separator", "-"),
                ("flyway.sqlMigrationSuffixes", ".pgsql"),
                ("flyway.placeholders.tenant", "acme"),
                ("flyway.locations", "filesystem:sql"),
                ("flyway.locations", "filesystem:sql"),
            ]),
            true,
        );
        assert_eq!(config.naming.versioned, "M");
        assert_eq!(config.naming.separator, "-");
        assert_eq!(config.naming.suffixes, [".pgsql"]);
        assert_eq!(config.naming.undo, "U");
        assert_eq!(config.placeholders, ["tenant"]);
        assert_eq!(config.locations, ["filesystem:sql"]);
    }

    #[test]
    fn defaults_to_flyway_locations() {
        assert_eq!(Config::from_settings(&[], false).locations, ["classpath:db/migration"]);
        assert_eq!(Config::from_settings(&[], true).locations, ["classpath:db/migration", "filesystem:sql"]);
    }

    #[test]
    fn locations_become_globs() {
        assert_eq!(location_glob("classpath:db/migration", "/project").as_deref(), Some("**/src/main/resources/db/migration/**/*"));
        assert_eq!(location_glob("db/migration/{vendor}", "/project").as_deref(), Some("**/src/main/resources/db/migration/*/**/*"));
        assert_eq!(location_glob("filesystem:./sql/", "/project").as_deref(), Some("sql/**/*"));
        assert_eq!(location_glob("filesystem:/project/db", "/project").as_deref(), Some("db/**/*"));
        assert_eq!(location_glob("filesystem:/elsewhere/db", "/project"), None);
        assert_eq!(location_glob("s3:bucket/migrations", "/project"), None);
    }

    #[test]
    fn finds_migrations_from_the_project_config() {
        let host = MockHost::install();
        host.add_file("flyway.conf", "flyway.locations=filesystem:db\n");
        host.add_file("db/V1__a.sql", "");
        host.add_file("db/notes.md", "");
        host.add_file("src/main/resources/application.properties", "spring.flyway.locations=classpath:db/spring\n");
        host.add_file("src/main/resources/db/spring/V2__b.sql", "");
        host.add_file("target/classes/db/spring/V2__b.sql", "");
        host.add_file("src/main/resources/db/migration/V9__default.sql", "");
        let project = host.invocation().project().unwrap();

        let config = load(&project);
        assert_eq!(config.locations, ["filesystem:db", "classpath:db/spring"]);
        assert_eq!(migration_files(&project, &config), ["/project/db/V1__a.sql", "/project/src/main/resources/db/spring/V2__b.sql"]);

        host.set_setting("locations", "\"filesystem:other, classpath:x\"");
        assert_eq!(load(&project).locations, ["filesystem:other", "classpath:x"]);
    }

    #[test]
    fn recognises_flyway_config_files() {
        assert!(is_flyway_config("/project/flyway.toml"));
        assert!(is_flyway_config("/project/conf/flyway-dev.conf"));
        assert!(!is_flyway_config("/project/app.conf"));
        assert!(!is_flyway_config("/project/flyway.sql"));
    }
}
