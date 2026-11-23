//! Run console highlights for Quarkus' default log format. Every pattern is
//! anchored to text only Quarkus (or a Java stack trace) prints, so other
//! programs' output keeps its colours.

use openidea_plugin::{ConsoleRule, host};

/// `2026-11-23 10:15:30,123 `, Quarkus' default `%d{yyyy-MM-dd HH:mm:ss,SSS}`.
const TIMESTAMP: &str = r"^\d{4}-\d\d-\d\d \d\d:\d\d:\d\d,\d{3} +";

pub(crate) fn rules() -> Vec<ConsoleRule> {
    vec![
        ConsoleRule::new(&format!(r"{TIMESTAMP}ERROR +\[")).foreground("red").style("bold"),
        ConsoleRule::new(&format!(r"{TIMESTAMP}WARN +\[")).foreground("yellow").style("bold"),
        ConsoleRule::new(r"^Caused by: [\w.$]+").foreground("red").style("bold"),
        ConsoleRule::new(r"\(powered by Quarkus [\w.-]+\) started in [\d.]+s").foreground("green").style("bold"),
    ]
}

pub(crate) fn apply() {
    let _ = host::set_console_rules(&rules());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuarkusTools;
    use openidea_plugin::Plugin;
    use openidea_plugin::mock::MockHost;
    use regex::Regex;

    fn matching(line: &str) -> Vec<String> {
        rules()
            .iter()
            .filter(|rule| Regex::new(&rule.pattern).unwrap().is_match(line))
            .map(|rule| rule.pattern.clone())
            .collect()
    }

    #[test]
    fn activation_sets_the_quarkus_console_rules() {
        let host = MockHost::install();
        QuarkusTools::default().activate(&host.context("aritti.quarkus")).unwrap();
        assert_eq!(host.console_rules(), rules());
    }

    #[test]
    fn rules_colour_quarkus_levels_failures_and_startup() {
        for line in [
            "2026-11-23 10:15:30,123 ERROR [io.qua.run.Application] (main) Failed to start application",
            "2026-11-23 10:15:30,123 WARN  [io.qua.config] (main) Unrecognized configuration key",
            "Caused by: java.lang.RuntimeException: boom",
            "2026-11-23 10:15:30,123 INFO  [io.quarkus] (Quarkus Main Thread) demo 1.0.0-SNAPSHOT on JVM (powered by Quarkus 3.17.0) started in 1.234s. Listening on: http://localhost:8080",
        ] {
            assert_eq!(matching(line).len(), 1, "{line}");
        }
    }

    #[test]
    fn rules_leave_other_output_alone() {
        for line in [
            "2026-11-23 10:15:30,123 INFO  [io.quarkus] (main) ERROR in the message is not a level",
            "2026-11-23T10:15:30.123+01:00 ERROR 12345 --- [main] c.e.Foo : spring boot line",
            "[ERROR] Failed to execute goal",
            "\tat com.example.Foo.bar(Foo.java:12)",
        ] {
            assert!(matching(line).is_empty(), "{line}: {:?}", matching(line));
        }
    }
}
