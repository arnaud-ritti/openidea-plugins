//! Run console highlights for Spring Boot's default log format and its
//! failure analysis block. Every pattern is anchored to text only Spring Boot
//! (or a Java stack trace) prints, so other programs' output keeps its colours.

use openidea_plugin::{ConsoleRule, host};

/// `2026-11-23T10:15:30.123+01:00 ` (Boot 3) or `2026-11-23 10:15:30.123  ` (Boot 2).
const TIMESTAMP: &str = r"^\d{4}-\d\d-\d\d[T ][\d:.+Z-]+ +";

pub(crate) fn rules() -> Vec<ConsoleRule> {
    vec![
        ConsoleRule::new(&format!(r"{TIMESTAMP}ERROR \d+ ---")).foreground("red").style("bold"),
        ConsoleRule::new(&format!(r"{TIMESTAMP}WARN \d+ ---")).foreground("yellow").style("bold"),
        ConsoleRule::new(r"^\*{27}$").foreground("red").whole_line(),
        ConsoleRule::new(r"^APPLICATION FAILED TO START$").foreground("red").style("bold").whole_line(),
        ConsoleRule::new(r"^Caused by: [\w.$]+").foreground("red").style("bold"),
        ConsoleRule::new(r"Started \w+ in [\d.]+ seconds").foreground("green").style("bold"),
        ConsoleRule::new(r"\w+ started on ports? \d+").foreground("green"),
    ]
}

pub(crate) fn apply() {
    let _ = host::set_console_rules(&rules());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BootTools;
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
    fn activation_sets_the_spring_boot_console_rules() {
        let host = MockHost::install();
        BootTools::default().activate(&host.context("aritti.spring-boot")).unwrap();
        assert_eq!(host.console_rules(), rules());
    }

    #[test]
    fn rules_colour_spring_boot_levels_failures_and_startup() {
        for line in [
            "2026-11-23T10:15:30.123+01:00 ERROR 12345 --- [demo] [main] c.e.Foo : boom",
            "2026-11-23T10:15:30.123Z ERROR 12345 --- [main] o.s.boot.SpringApplication : boom",
            "2026-11-23 10:15:30.123 ERROR 12345 --- [main] o.s.boot.SpringApplication : boom",
            "2026-11-23T10:15:30.123+01:00  WARN 12345 --- [demo] [main] c.e.Foo : careful",
            "***************************",
            "APPLICATION FAILED TO START",
            "Caused by: java.lang.IllegalStateException: no bean",
            "2026-11-23T10:15:30.123+01:00  INFO 1 --- [main] c.e.DemoApplication : Started DemoApplication in 1.234 seconds (process running for 1.5)",
            "2026-11-23T10:15:30.123+01:00  INFO 1 --- [main] o.s.b.w.e.tomcat.TomcatWebServer : Tomcat started on port 8080 (http) with context path '/'",
        ] {
            assert_eq!(matching(line).len(), 1, "{line}");
        }
    }

    #[test]
    fn rules_leave_other_output_alone() {
        for line in [
            "2026-11-23T10:15:30.123+01:00  INFO 12345 --- [demo] [main] c.e.Foo : ERROR in the message is not a level",
            "ERROR: something unrelated",
            "[ERROR] Failed to execute goal",
            "2026-11-23 10:15:30,123 ERROR [io.quarkus] (main) Failed to start",
            "\tat com.example.Foo.bar(Foo.java:12)",
            "****",
        ] {
            assert!(matching(line).is_empty(), "{line}: {:?}", matching(line));
        }
    }
}
