//! Run console colours (API 1.12) for Symfony output: Monolog levels (log files, the console
//! handler and the Symfony CLI server log), PHP errors, SymfonyStyle blocks, exception headers and
//! vendor trace frames. They apply in every window, so they are set only while a Symfony project is
//! open.
use crate::Main;
use openidea_plugin::{ConsoleRule, LogLevel, host};

/// Whole-line rules first (the first one that matches styles the line), then match-only ones.
pub(crate) fn rules() -> Vec<ConsoleRule> {
    vec![
        ConsoleRule::new(r"^\[\d{4}-\d{2}-\d{2}[ T][^\]]*\] [\w-]+\.(EMERGENCY|ALERT|CRITICAL|ERROR):").foreground("red").whole_line(),
        ConsoleRule::new(r"^\[\d{4}-\d{2}-\d{2}[ T][^\]]*\] [\w-]+\.WARNING:").foreground("yellow").whole_line(),
        ConsoleRule::new(r"^\d{2}:\d{2}:\d{2} (EMERGENCY|ALERT|CRITICAL|ERROR) +\[").foreground("red").whole_line(),
        ConsoleRule::new(r"^\d{2}:\d{2}:\d{2} WARNING +\[").foreground("yellow").whole_line(),
        ConsoleRule::new(r"^\[[^\]]*\] [^|]*\|(EMERGENCY|ALERT|CRITICAL|ERROR) *\|").foreground("red").whole_line(),
        ConsoleRule::new(r"^\[[^\]]*\] [^|]*\|WARNING *\|").foreground("yellow").whole_line(),
        ConsoleRule::new(r"^(PHP )?(Fatal|Parse) error: ").foreground("red").style("bold").whole_line(),
        ConsoleRule::new(r"^(PHP )?(Warning|Deprecated|Notice): ").foreground("yellow").whole_line(),
        ConsoleRule::new(r"^ *\[(ERROR|CAUTION)\] ").foreground("red").style("bold").whole_line(),
        ConsoleRule::new(r"^ *\[WARNING\] ").foreground("yellow").whole_line(),
        ConsoleRule::new(r"^ *\[OK\] ").foreground("green").whole_line(),
        ConsoleRule::new(r"^In [\w.-]+ line \d+:").foreground("red").style("bold").whole_line(),
        ConsoleRule::new(r"^ ( *at|[^ ]+ at) [^ ]*/vendor/").style("dim").whole_line(),
        ConsoleRule::new(r"^ *\[[A-Z][\w\\]*(Exception|Error)\] *$").foreground("red").style("bold"),
    ]
}

impl Main {
    /// Sets the rules when a Symfony project is indexed and removes them otherwise; only on change.
    pub(crate) fn show_console_rules(&mut self, symfony_open: bool) {
        if symfony_open == self.console_rules {
            return;
        }
        let rules = if symfony_open { rules() } else { Vec::new() };
        match host::set_console_rules(&rules) {
            Ok(()) => self.console_rules = symfony_open,
            Err(error) => host::log(LogLevel::Warn, &format!("Symfony console colours: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::activated;
    use openidea_plugin::Plugin;
    use openidea_plugin::mock::MockHost;

    #[test]
    fn rules_are_set_while_a_symfony_project_is_indexed_and_removed_after() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        assert_eq!(host.console_rules(), rules());

        host.add_file("composer.json", r#"{"require": {"laravel/framework": "^11.0"}}"#);
        plugin.run_command("reindex", &host.invocation(), None).unwrap();
        assert!(host.console_rules().is_empty());
    }

    #[test]
    fn rules_stay_within_the_host_limits() {
        let rules = rules();
        assert!(rules.len() <= 100);
        assert!(rules.iter().all(|rule| !rule.pattern.is_empty() && rule.pattern.len() <= 512 && rule.enabled));
        assert!(rules.iter().take_while(|rule| rule.whole_line).count() == rules.len() - 1, "match-only rules come last");
    }
}
