//! Run console highlights for Liquibase's own messages, from the CLI or a framework's
//! log (Spring Boot runs Liquibase at startup). Set once a changelog or a
//! liquibase.properties is seen.

use openidea_plugin::{ConsoleRule, host};

pub(crate) fn rules() -> Vec<ConsoleRule> {
    vec![
        ConsoleRule::new(r"^ERROR: ").foreground("red").whole_line(),
        ConsoleRule::new(r"Unexpected error running Liquibase: ").foreground("red").style("bold"),
        ConsoleRule::new(r"Validation Failed:").foreground("red").style("bold"),
        ConsoleRule::new(r"Liquibase command '[\w-]+' was executed successfully").foreground("green").style("bold"),
        ConsoleRule::new(r"Update has been successful").foreground("green"),
        ConsoleRule::new(r"No validation errors found").foreground("green"),
        ConsoleRule::new(r"ChangeSet .* ran successfully").foreground("green"),
        ConsoleRule::new(r"Running Changeset: ").foreground("cyan"),
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
    fn rules_colour_liquibase_cli_and_log_messages() {
        for line in [
            "ERROR: Exception Primary Class:  DatabaseException",
            "ERROR: Exception Primary Reason:  Table 'users' already exists",
            "Unexpected error running Liquibase: Validation Failed:",
            "liquibase.exception.ValidationFailedException: Validation Failed:",
            "Liquibase command 'update' was executed successfully.",
            "Liquibase command 'rollback-count' was executed successfully.",
            "Liquibase: Update has been successful. Rows affected: 3",
            "No validation errors found.",
            "2026-11-23T10:15:30.123+01:00  INFO 1 --- [main] liquibase.changelog : ChangeSet db/changelog/users.yaml::1::alice ran successfully in 12ms",
            "Running Changeset: db/changelog/users.yaml::1::alice",
        ] {
            assert!(!matching(line).is_empty(), "{line}");
        }
    }

    #[test]
    fn rules_leave_other_output_alone() {
        for line in [
            "[ERROR] Failed to execute goal",
            "2026-11-23 10:15:30 ERROR something else",
            "Starting Liquibase at 10:15:30 (version 4.29.0)",
            "Liquibase Version: 4.29.0",
            "UPDATE SUMMARY",
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

    #[test]
    fn rules_are_set_once_a_changelog_is_seen() {
        let host = openidea_plugin::mock::MockHost::install();
        let mut live = crate::live::State::default();
        host.add_file("config/app.yaml", "server:\n  port: 8080\n");
        live.update("/project", "/project/config/app.yaml", "server:\n  port: 8080\n");
        assert!(host.console_rules().is_empty());
        host.add_file("db/changelog.yaml", "databaseChangeLog:\n  - changeSet:\n      id: one\n      author: a\n");
        live.update("/project", "/project/db/changelog.yaml", "databaseChangeLog:\n  - changeSet:\n      id: one\n      author: a\n");
        assert_eq!(host.console_rules(), rules());
    }
}
