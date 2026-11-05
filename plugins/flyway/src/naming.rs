//! Flyway migration file names: `V1_1__add_users.sql`, `R__views.sql`.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Naming {
    pub versioned: String,
    pub undo: String,
    pub repeatable: String,
    pub baseline: String,
    pub separator: String,
    pub suffixes: Vec<String>,
}

impl Default for Naming {
    fn default() -> Self {
        Naming {
            versioned: "V".into(),
            undo: "U".into(),
            repeatable: "R".into(),
            baseline: "B".into(),
            separator: "__".into(),
            suffixes: vec![".sql".into()],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Versioned,
    Undo,
    Baseline,
    Repeatable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Name {
    Migration { kind: Kind, version: String, description: String },
    Callback,
    NotMigration,
    Invalid(String),
}

fn invalid(reason: String) -> Name {
    Name::Invalid(format!("Flyway skips this file: {reason}"))
}

fn is_version(text: &str) -> bool {
    text.split(['.', '_']).all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

pub fn parse(file_name: &str, naming: &Naming) -> Name {
    let Some(stem) = naming.suffixes.iter().find_map(|suffix| file_name.strip_suffix(suffix.as_str())) else {
        return Name::NotMigration;
    };
    // ponytail: a prefix test, not Flyway's callback event list.
    if stem.starts_with("before") || stem.starts_with("after") {
        return Name::Callback;
    }

    let sep = naming.separator.as_str();
    let suffix = naming.suffixes.first().map_or("", String::as_str);
    let example = format!("{}1{sep}add_users{suffix}", naming.versioned);
    let unknown = || {
        invalid(format!(
            "migration names start with {}, {}, {} or {}, like {example}.",
            naming.versioned, naming.undo, naming.repeatable, naming.baseline
        ))
    };

    let mut prefixes = [
        (&naming.versioned, Kind::Versioned),
        (&naming.undo, Kind::Undo),
        (&naming.baseline, Kind::Baseline),
        (&naming.repeatable, Kind::Repeatable),
    ];
    prefixes.sort_by_key(|(prefix, _)| std::cmp::Reverse(prefix.len()));
    let Some((rest, kind)) =
        prefixes.iter().find_map(|(prefix, kind)| stem.strip_prefix(prefix.as_str()).map(|rest| (rest, *kind)))
    else {
        return unknown();
    };

    if kind == Kind::Repeatable {
        if let Some(description) = rest.strip_prefix(sep) {
            return Name::Migration { kind, version: String::new(), description: description.into() };
        }
        if rest.starts_with(|c: char| c.is_ascii_digit()) {
            return invalid(format!(
                "repeatable migrations have no version, like {}{sep}refresh_views{suffix}.",
                naming.repeatable
            ));
        }
        return unknown();
    }

    let Some((version, description)) = rest.split_once(sep) else {
        if is_version(rest) {
            return Name::Migration { kind, version: rest.into(), description: String::new() };
        }
        if !rest.starts_with(|c: char| c.is_ascii_digit()) {
            return unknown();
        }
        return invalid(format!("no `{sep}` between the version and the description, like {example}."));
    };
    if version.is_empty() {
        return invalid(format!("the version is missing, like {example}."));
    }
    if !is_version(version) {
        return invalid(format!("`{version}` isn't a version (numbers separated by dots or underscores)."));
    }
    Name::Migration { kind, version: version.into(), description: description.into() }
}

/// Parts without leading zeros and trailing zero parts, so `1_0`, `1.0` and `01` compare equal.
pub fn version_key(version: &str) -> Vec<String> {
    let mut parts: Vec<String> = version
        .split(['.', '_'])
        .map(|part| match part.trim_start_matches('0') {
            "" => "0".to_string(),
            trimmed => trimmed.to_string(),
        })
        .collect();
    while parts.last().is_some_and(|part| part == "0") {
        parts.pop();
    }
    parts
}

pub fn next_version(versions: &[&str]) -> String {
    let numeric_key = |version: &str| -> Vec<(usize, String)> {
        version_key(version).into_iter().map(|part| (part.len(), part)).collect()
    };
    let Some(highest) = versions.iter().max_by_key(|version| numeric_key(version)) else {
        return "1".into();
    };
    let (head, last) = highest.split_at(highest.rfind(['.', '_']).map_or(0, |i| i + 1));
    // ponytail: u128 caps the last part at 38 digits; real versions are dates or small counters.
    let bumped = last.parse::<u128>().map_or(1, |n| n + 1);
    format!("{head}{bumped:0width$}", width = last.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn migration(kind: Kind, version: &str, description: &str) -> Name {
        Name::Migration { kind, version: version.into(), description: description.into() }
    }

    #[test]
    fn parses_versioned_undo_baseline_and_repeatable() {
        let n = Naming::default();
        assert_eq!(parse("V1_1__add_users.sql", &n), migration(Kind::Versioned, "1_1", "add_users"));
        assert_eq!(parse("U2__drop.sql", &n), migration(Kind::Undo, "2", "drop"));
        assert_eq!(parse("B3__baseline.sql", &n), migration(Kind::Baseline, "3", "baseline"));
        assert_eq!(parse("R__views.sql", &n), migration(Kind::Repeatable, "", "views"));
        assert_eq!(parse("V4.sql", &n), migration(Kind::Versioned, "4", ""));
    }

    #[test]
    fn explains_names_flyway_skips() {
        let n = Naming::default();
        let msg = |name: &str| match parse(name, &n) {
            Name::Invalid(m) => m,
            other => panic!("{name}: {other:?}"),
        };
        let unknown = "Flyway skips this file: migration names start with V, U, R or B, like V1__add_users.sql.";
        assert_eq!(msg("V1_init.sql"), "Flyway skips this file: no `__` between the version and the description, like V1__add_users.sql.");
        assert_eq!(msg("V1a__init.sql"), "Flyway skips this file: `1a` isn't a version (numbers separated by dots or underscores).");
        assert_eq!(msg("V1..2__init.sql"), "Flyway skips this file: `1..2` isn't a version (numbers separated by dots or underscores).");
        assert_eq!(msg("V__init.sql"), "Flyway skips this file: the version is missing, like V1__add_users.sql.");
        assert_eq!(msg("R1__views.sql"), "Flyway skips this file: repeatable migrations have no version, like R__refresh_views.sql.");
        assert_eq!(msg("init.sql"), unknown);
        assert_eq!(msg("v1__init.sql"), unknown);
        assert_eq!(msg("Views.sql"), unknown);
    }

    #[test]
    fn skips_callbacks_and_other_files() {
        let n = Naming::default();
        assert_eq!(parse("beforeMigrate.sql", &n), Name::Callback);
        assert_eq!(parse("afterMigrate__seed.sql", &n), Name::Callback);
        assert_eq!(parse("README.md", &n), Name::NotMigration);
        assert_eq!(parse("V1__x.sql.conf", &n), Name::NotMigration);
    }

    #[test]
    fn uses_configured_naming() {
        let n = Naming { versioned: "M".into(), separator: "-".into(), suffixes: vec![".pgsql".into()], ..Naming::default() };
        assert_eq!(parse("M7-add.pgsql", &n), migration(Kind::Versioned, "7", "add"));
        assert_eq!(parse("V7__add.sql", &n), Name::NotMigration);
        let Name::Invalid(m) = parse("X7-add.pgsql", &n) else { panic!() };
        assert_eq!(m, "Flyway skips this file: migration names start with M, U, R or B, like M1-add_users.pgsql.");
    }

    #[test]
    fn equal_versions_share_a_key() {
        assert_eq!(version_key("1_1"), version_key("1.1"));
        assert_eq!(version_key("01"), version_key("1"));
        assert_eq!(version_key("1.0"), version_key("1"));
        assert_ne!(version_key("1.10"), version_key("1.1"));
        assert_ne!(version_key("2"), version_key("20"));
    }

    #[test]
    fn next_version_bumps_the_highest() {
        assert_eq!(next_version(&[]), "1");
        assert_eq!(next_version(&["1", "10", "9"]), "11");
        assert_eq!(next_version(&["1.2", "1.10"]), "1.11");
        assert_eq!(next_version(&["0099"]), "0100");
        assert_eq!(next_version(&["2_1", "2"]), "2_2");
    }
}
