//! Run console highlights for Flyway's own messages, from the CLI or a framework's
//! log (Spring Boot runs Flyway at startup). Set once a project is seen to use Flyway.

use openidea_plugin::{ConsoleRule, host};

pub(crate) fn rules() -> Vec<ConsoleRule> {
    vec![
        ConsoleRule::new(r"^ERROR: ").foreground("red").whole_line(),
        ConsoleRule::new(r"^WARNING: ").foreground("yellow").whole_line(),
        ConsoleRule::new(r"Migration of schema .* failed!").foreground("red").style("bold"),
        ConsoleRule::new(r"Validate failed").foreground("red").style("bold"),
        ConsoleRule::new(r"Successfully applied \d+ migrations? to schema").foreground("green").style("bold"),
        ConsoleRule::new(r"Successfully validated \d+ migrations?").foreground("green"),
        ConsoleRule::new(r"Successfully repaired schema history table").foreground("green"),
        ConsoleRule::new(r"is up to date\. No migration necessary\.").foreground("green"),
        ConsoleRule::new(r"Migrating schema \S+ (to version|with repeatable migration) ").foreground("cyan"),
    ]
}

pub(crate) fn apply() {
    let _ = host::set_console_rules(&rules());
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    fn matching(line: &str) -> Vec<String> {
        rules().iter().filter(|rule| Regex::new(&rule.pattern).unwrap().is_match(line)).map(|rule| rule.pattern.clone()).collect()
    }

    #[test]
    fn rules_colour_flyway_cli_and_log_messages() {
        for line in [
            "ERROR: Validate failed: Migrations have failed validation",
            "ERROR: Unable to connect to the database",
            "WARNING: No locations configured",
            "2026-11-23T10:15:30.123+01:00  INFO 1 --- [main] o.f.core.internal.command.DbMigrate : Successfully applied 2 migrations to schema \"public\", now at version v2 (execution time 00:00.012s)",
            "Successfully applied 1 migration to schema \"PUBLIC\", now at version v1 (execution time 00:00.004s)",
            "Successfully validated 3 migrations (execution time 00:00.010s)",
            "Successfully repaired schema history table \"PUBLIC\".\"flyway_schema_history\" (execution time 00:00.020s).",
            "Schema \"PUBLIC\" is up to date. No migration necessary.",
            "Migrating schema \"PUBLIC\" to version \"2 - add users\"",
            "Migrating schema \"PUBLIC\" with repeatable migration \"views\"",
            "o.f.core.internal.command.DbMigrate : Migration of schema \"public\" to version \"3 - broken\" failed! Please restore backups and roll back database and code!",
        ] {
            assert!(!matching(line).is_empty(), "{line}");
        }
    }

    #[test]
    fn rules_leave_other_output_alone() {
        for line in [
            "[ERROR] Failed to execute goal",
            "2026-11-23 10:15:30 ERROR something else",
            "Flyway OSS Edition 10.0.0 by Redgate",
            "Database: jdbc:h2:mem:test (H2 2.2)",
            "Current version of schema \"PUBLIC\": 1",
            "Successfully started the server",
        ] {
            assert!(matching(line).is_empty(), "{line}: {:?}", matching(line));
        }
    }

    #[test]
    fn rules_fit_the_host_limits() {
        let host = openidea_plugin::mock::MockHost::install();
        apply();
        assert_eq!(host.console_rules(), rules());
    }
}
