//! Completion of `${placeholders}` in SQL and of keys in Flyway config files.

use openidea_plugin::providers::{CompletionItem, CompletionKind};
use openidea_plugin::text::position;
use openidea_plugin::Range;

pub const DEFAULT_PLACEHOLDERS: &[(&str, &str)] = &[
    ("flyway:defaultSchema", "Default schema"),
    ("flyway:user", "Connecting user"),
    ("flyway:database", "Database name from the URL"),
    ("flyway:timestamp", "When the script was parsed (yyyy-MM-dd HH:mm:ss)"),
    ("flyway:filename", "File name of this script"),
    ("flyway:workingDirectory", "Working directory (user.dir)"),
    ("flyway:table", "Schema history table"),
    ("flyway:environment", "Environment name"),
];

pub const FLYWAY_KEYS: &[&str] = &[
    "baselineDescription", "baselineMigrationPrefix", "baselineOnMigrate", "baselineVersion", "batch",
    "callbackLocations", "callbacks", "cherryPick", "cleanDisabled", "communityDBSupportEnabled",
    "createSchemas", "defaultSchema", "detectEncoding", "dryRunOutput", "encoding", "environment",
    "errorOverrides", "executeInTransaction", "failOnMissingLocations", "group", "ignoreMigrationPatterns",
    "installedBy", "jarDirs", "kerberosConfigFile", "locations", "lockRetryCount", "loggers",
    "migrationResolvers", "mixed", "outOfOrder", "outputQueryResults", "placeholderPrefix",
    "placeholderReplacement", "placeholderSeparator", "placeholderSuffix", "repeatableSqlMigrationPrefix",
    "reportEnabled", "reportFilename", "schemaModelLocation", "schemaModelSchemas", "scriptPlaceholderPrefix",
    "scriptPlaceholderSuffix", "skipDefaultCallbacks", "skipDefaultResolvers", "skipExecutingMigrations",
    "sqlMigrationPrefix", "sqlMigrationSeparator", "sqlMigrationSuffixes", "stream", "table", "tablespace",
    "target", "undoSqlMigrationPrefix", "validateMigrationNaming", "validateOnMigrate",
];

pub const ENVIRONMENT_KEYS: &[&str] = &[
    "url", "user", "password", "driver", "schemas", "connectRetries", "connectRetriesInterval", "initSql",
];

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |newline| newline + 1)
}

fn replacing(text: &str, from: usize, at: usize, item: &mut CompletionItem) {
    item.replace = Some(Range::new(position(text, from), position(text, at)));
}

pub fn placeholders(text: &str, at: usize, project: &[String]) -> Vec<CompletionItem> {
    let before = &text[line_start(text, at)..at];
    // ponytail: ${ … } only; custom placeholderPrefix/Suffix are not read
    let Some(open) = before.rfind("${") else { return Vec::new() };
    let typed = &before[open + 2..];
    if !typed.chars().all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c)) {
        return Vec::new();
    }

    let from = at - typed.len();
    let end = if text[at..].starts_with('}') { at + 1 } else { at };
    let defaults = DEFAULT_PLACEHOLDERS.iter().map(|(name, detail)| (*name, *detail, CompletionKind::Constant));
    let custom = project
        .iter()
        .filter(|name| !DEFAULT_PLACEHOLDERS.iter().any(|(default, _)| default == name))
        .map(|name| (name.as_str(), "Placeholder", CompletionKind::Variable));

    defaults
        .chain(custom)
        .map(|(name, detail, kind)| {
            let mut item = CompletionItem::new(name, kind);
            item.detail = Some(detail.to_owned());
            item.insert_text = format!("{name}}}");
            item.filter_text = Some(name.to_owned());
            replacing(text, from, end, &mut item);
            item
        })
        .collect()
}

fn key_items(
    text: &str,
    at: usize,
    keys: impl Iterator<Item = String>,
    insert: impl Fn(&str) -> String,
) -> Vec<CompletionItem> {
    let start = line_start(text, at);
    let before = &text[start..at];
    let typed = before.trim_start();
    if !typed.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
        return Vec::new();
    }

    let from = start + (before.len() - typed.len());
    keys.map(|key| {
        let mut item = CompletionItem::new(&key, CompletionKind::Property);
        item.insert_text = insert(&key);
        replacing(text, from, at, &mut item);
        item
    })
    .collect()
}

pub fn conf_keys(text: &str, at: usize) -> Vec<CompletionItem> {
    let keys = FLYWAY_KEYS.iter().chain(ENVIRONMENT_KEYS).map(|key| format!("flyway.{key}"));
    key_items(text, at, keys, |key| format!("{key}="))
}

pub fn toml_keys(text: &str, at: usize) -> Vec<CompletionItem> {
    let table = text[..line_start(text, at)]
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| line.starts_with('['))
        .map(|line| line.trim_matches(|c| c == '[' || c == ']').trim());

    let keys: &[&str] = match table {
        Some("flyway") => FLYWAY_KEYS,
        Some(table) if table.strip_prefix("environments.").is_some_and(|name| !name.is_empty() && !name.contains('.')) => {
            ENVIRONMENT_KEYS
        }
        _ => return Vec::new(),
    };

    key_items(text, at, keys.iter().map(|key| (*key).to_owned()), |key| format!("{key} = "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::{Position, Range};

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    #[test]
    fn completes_placeholders_after_the_prefix() {
        let text = "create schema ${fl";
        let items = placeholders(text, text.len(), &["tenant".into()]);
        assert_eq!(labels(&items)[..2], ["flyway:defaultSchema", "flyway:user"]);
        assert!(labels(&items).contains(&"tenant"));
        let user = items.iter().find(|i| i.label == "flyway:user").unwrap();
        assert_eq!(user.insert_text, "flyway:user}");
        assert_eq!(user.replace, Some(Range::new(Position::new(0, 16), Position::new(0, 18))));

        let closed = "select ${fl}";
        let items = placeholders(closed, 11, &[]);
        let user = items.iter().find(|i| i.label == "flyway:user").unwrap();
        assert_eq!(user.replace, Some(Range::new(Position::new(0, 9), Position::new(0, 12))));

        assert!(placeholders("select '${done}' ", 17, &[]).is_empty(), "closed placeholder");
        assert!(placeholders("select 1", 8, &[]).is_empty());
    }

    #[test]
    fn completes_keys_in_flyway_conf() {
        let text = "flyway.url=x\nflyway.loc";
        let items = conf_keys(text, text.len());
        let item = items.iter().find(|i| i.label == "flyway.locations").unwrap();
        assert_eq!(item.insert_text, "flyway.locations=");
        assert_eq!(item.replace, Some(Range::new(Position::new(1, 0), Position::new(1, 10))));
        assert!(labels(&items).contains(&"flyway.url"));
        assert!(conf_keys("flyway.url=jdbc", 15).is_empty(), "in a value");
    }

    #[test]
    fn completes_keys_by_toml_table() {
        let flyway = "[flyway]\nloc";
        let items = toml_keys(flyway, flyway.len());
        assert_eq!(items.iter().find(|i| i.label == "locations").unwrap().insert_text, "locations = ");
        assert!(!labels(&items).contains(&"url"));

        let env = "[environments.dev]\nur";
        assert!(labels(&toml_keys(env, env.len())).contains(&"url"));

        let other = "[flyway.placeholders]\nte";
        assert!(toml_keys(other, other.len()).is_empty());
    }
}
