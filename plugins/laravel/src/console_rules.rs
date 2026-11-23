//! Run console colours (API 1.12) for Laravel output: log levels, PHP errors, Artisan and Pest
//! labels, exception headers and stack frames. They apply in every window, so they are set only
//! while a Laravel project is open.
use crate::Main;
use openidea_plugin::{ConsoleRule, LogLevel, host};

/// Whole-line rules first (the first one that matches styles the line), then match-only ones.
pub(crate) fn rules() -> Vec<ConsoleRule> {
    vec![
        ConsoleRule::new(r"^\[\d{4}-\d{2}-\d{2}[ T][^\]]*\] [\w-]+\.(EMERGENCY|ALERT|CRITICAL|ERROR):").foreground("red").whole_line(),
        ConsoleRule::new(r"^\[\d{4}-\d{2}-\d{2}[ T][^\]]*\] [\w-]+\.WARNING:").foreground("yellow").whole_line(),
        ConsoleRule::new(r"^(PHP )?(Fatal|Parse) error: ").foreground("red").style("bold").whole_line(),
        ConsoleRule::new(r"^(PHP )?(Warning|Deprecated|Notice): ").foreground("yellow").whole_line(),
        ConsoleRule::new(r"^ +(ERROR|FAIL)  ").foreground("red").style("bold").whole_line(),
        ConsoleRule::new(r"^ +WARN  ").foreground("yellow").whole_line(),
        ConsoleRule::new(r"^#\d+ [^ ]*/vendor/").style("dim").whole_line(),
        ConsoleRule::new(r"^ +\d+ +vendor/").style("dim").whole_line(),
        ConsoleRule::new(r"^ +[A-Z][\w\\]*(Exception|Error) *$").foreground("red").style("bold"),
        ConsoleRule::new(r"^ +(at |\d+ +)(app|bootstrap|config|database|resources|routes|tests)/[\w/-]*(\.blade)?\.php:\d+").foreground("cyan").style("underline"),
        ConsoleRule::new(r"^ +PASS  ").foreground("green").style("bold"),
        ConsoleRule::new(r"\.{3} [\d.]+m?s DONE$").foreground("green"),
        ConsoleRule::new(r"\.{3} [\d.]+m?s FAIL$").foreground("red"),
    ]
}

impl Main {
    /// Sets the rules when a Laravel project is open and removes them otherwise; only on change.
    pub(crate) fn show_console_rules(&mut self, laravel_open: bool) {
        if laravel_open == self.console_rules {
            return;
        }
        let rules = if laravel_open { rules() } else { Vec::new() };
        match host::set_console_rules(&rules) {
            Ok(()) => self.console_rules = laravel_open,
            Err(error) => host::log(LogLevel::Warn, &format!("Laravel console colours: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::Plugin;
    use openidea_plugin::mock::MockHost;

    fn styled(line: &str) -> Vec<usize> {
        rules().iter().enumerate().filter(|(_, rule)| regex::Regex::new(&rule.pattern).unwrap().is_match(line)).map(|(i, _)| i).collect()
    }

    #[test]
    fn rules_colour_laravel_output_and_leave_ordinary_lines_alone() {
        assert_eq!(styled("[2026-11-23 10:00:00] local.ERROR: SQLSTATE[HY000] Connection refused"), [0]);
        assert_eq!(styled("[2026-11-23T10:00:00.123456+00:00] production.CRITICAL: Boom"), [0]);
        assert_eq!(styled("[2026-11-23 10:00:00] local.WARNING: Slow query"), [1]);
        assert!(styled("[2026-11-23 10:00:00] local.INFO: Logged in").is_empty());
        assert_eq!(styled("PHP Fatal error:  Uncaught Error: Class \"Foo\" not found"), [2]);
        assert_eq!(styled("Parse error: syntax error, unexpected '}'"), [2]);
        assert_eq!(styled("PHP Deprecated:  Implicit conversion"), [3]);
        assert_eq!(styled("   ERROR  Command \"foo\" is not defined."), [4]);
        assert_eq!(styled("   FAIL  Tests\\Feature\\ExampleTest"), [4]);
        assert_eq!(styled("   WARN  The migration table is missing."), [5]);
        assert_eq!(styled("#12 /var/www/html/vendor/laravel/framework/src/Illuminate/Pipeline/Pipeline.php(183): handle()"), [6]);
        assert_eq!(styled("  14  vendor/laravel/framework/src/Illuminate/Database/Connection.php:771"), [7]);
        assert_eq!(styled("   Illuminate\\Database\\QueryException "), [8]);
        assert_eq!(styled("  at app/Http/Controllers/PostController.php:25"), [9]);
        assert_eq!(styled("  at resources/views/posts/index.blade.php:3"), [9]);
        assert_eq!(styled("  5   app/Models/Post.php:12"), [9]);
        assert_eq!(styled("   PASS  Tests\\Unit\\ExampleTest"), [10]);
        assert_eq!(styled("  2014_10_12_000000_create_users_table ............ 12.50ms DONE"), [11]);
        for ordinary in [
            "   INFO  Server running on [http://127.0.0.1:8000].",
            "  2026-11-23 10:00:00 / ...................................... ~ 0.5ms",
            "Error handling is configured in bootstrap/app.php",
            "#0 /var/www/html/app/Http/Controllers/PostController.php(25): index()",
            "vendor/app/Thing.php:12",
            "Seeding DONE",
            "Build FAIL",
        ] {
            assert!(styled(ordinary).is_empty(), "{ordinary}");
        }
    }

    #[test]
    fn rules_are_set_once_while_a_laravel_project_is_open_and_removed_after() {
        let host = MockHost::install();
        let mut plugin = Main::default();
        plugin.show_console_rules(true);
        assert_eq!(host.console_rules(), rules());
        plugin.show_console_rules(true);
        plugin.show_console_rules(false);
        assert!(host.console_rules().is_empty());
    }

    #[test]
    fn activation_sets_rules_only_in_a_laravel_project() {
        let host = MockHost::install();
        host.add_file("composer.json", r#"{"require": {"symfony/framework-bundle": "^7.0"}}"#);
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.laravel")).unwrap();
        assert!(host.console_rules().is_empty());

        host.add_file("artisan", "#!/usr/bin/env php\n");
        plugin.run_command("reindex", &host.invocation(), None).unwrap();
        assert_eq!(host.console_rules().len(), rules().len());
    }
}
