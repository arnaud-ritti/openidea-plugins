//! Checks of YAML and XML configuration files: factory settings Symfony 3.0
//! removed, and plain YAML scalars the parser refuses or reads differently.

use crate::checks::{Finding, is_service_file};
use crate::index::{self, xml_attribute, xml_tags};
use crate::{is_quoted_config, yaml_quotes};

const FACTORY_SETTINGS: [(&str, &str); 3] = [("factory_class", "factory-class"), ("factory_method", "factory-method"), ("factory_service", "factory-service")];

fn deprecated_factory(range: std::ops::Range<usize>, name: &str) -> Finding {
    let message = format!("The \"{name}\" setting was removed in Symfony 3.0: use \"factory\" instead.");
    Finding::warn(range, "deprecated-factory-setting", message)
}

pub fn deprecated_factory_settings(path: &str, language: &str, text: &str) -> Vec<Finding> {
    let mut found = Vec::new();
    if !is_service_file(path) {
        return found;
    }

    if language == "yaml" {
        for key in index::yaml_keys(text) {
            let is_service_key = key.parents.len() == 2 && key.parents[0] == "services";
            if is_service_key && FACTORY_SETTINGS.iter().any(|(yaml, _)| *yaml == key.key) {
                found.push(deprecated_factory(key.offset..key.offset + key.key.len(), &key.key));
            }
        }
    }

    if language == "xml" && text.contains("<container") {
        for (tag, at) in xml_tags(text, "service") {
            for (_, name) in FACTORY_SETTINGS {
                if let Some((_, value_at)) = xml_attribute(tag, name) {
                    let start = at + value_at - 2 - name.len();
                    found.push(deprecated_factory(start..start + name.len(), name));
                }
            }
        }
    }

    found.sort_by_key(|finding| finding.start);
    found
}

pub fn yaml_scalars(path: &str, language: &str, text: &str) -> Vec<Finding> {
    let mut found = Vec::new();
    if language != "yaml" || !is_quoted_config(path) {
        return found;
    }

    for range in yaml_quotes::unquoted(text) {
        let first = text[range.start..].chars().next().unwrap_or('@');
        found.push(Finding::warn(range, "unquoted-yaml-scalar", format!("A YAML value starting with \"{first}\" must be quoted.")));
    }

    for range in yaml_quotes::backslashes(text) {
        let message = "A backslash in a double-quoted YAML string starts an escape sequence: write \\\\ for a literal backslash.".to_owned();
        found.push(Finding { replacement: Some("\\\\".into()), ..Finding::hint(range, "yaml-unescaped-backslash", message) });
    }

    for range in yaml_quotes::colons(text) {
        let message = "A \": \" in an unquoted YAML value is read as a nested mapping: quote the value.".to_owned();
        found.push(Finding::hint(range, "yaml-unquoted-colon", message));
    }

    found.sort_by_key(|finding| finding.start);
    found
}

#[cfg(test)]
mod tests {
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::{Diagnostic, Event, Plugin, Position, Range as TextRange};

    use crate::index::Index;
    use crate::{Groups, Main};

    const SERVICES: &str = "/project/config/services.yaml";

    fn run_in(path: &str, language: &str, text: &str, groups: Groups) -> Vec<Diagnostic> {
        let read = |_: &str| None;
        crate::extra_problems(&Index::default(), groups, &read, path, language, text)
    }

    fn yaml(text: &str) -> Vec<Diagnostic> {
        run_in(SERVICES, "yaml", text, Groups::all_on())
    }

    fn codes(found: &[Diagnostic]) -> Vec<&str> {
        found.iter().map(|d| d.code.as_deref().unwrap_or_default()).collect()
    }

    #[test]
    fn deprecated_factory_setting_warns_in_yaml() {
        let text = "services:\n    app.a:\n        class: App\\A\n        factory_class: App\\F\n        factory_method: create\n";
        let found = yaml(text);
        assert_eq!(codes(&found), ["deprecated-factory-setting", "deprecated-factory-setting"]);
        assert_eq!(found[1].range, TextRange::new(Position::new(4, 8), Position::new(4, 22)));
        assert!(found[1].message.contains("factory") && found[1].message.contains("3.0"));
    }

    #[test]
    fn deprecated_factory_setting_warns_in_xml() {
        let text = "<container>\n  <services>\n    <service id=\"a\" class=\"A\" factory-class=\"F\" factory-method=\"m\"/>\n  </services>\n</container>";
        let found = run_in("/project/config/services.xml", "xml", text, Groups::all_on());
        assert_eq!(codes(&found), ["deprecated-factory-setting", "deprecated-factory-setting"]);
        assert_eq!(found[0].range, TextRange::new(Position::new(2, 30), Position::new(2, 43)));
    }

    #[test]
    fn deprecated_factory_setting_is_quiet_for_the_factory_key_and_other_sections() {
        let text = "parameters:\n    factory_method: x\nservices:\n    app.a:\n        factory: ['@f', create]\n";
        assert!(yaml(text).is_empty());
        let xml = "<container><services><service id=\"a\"><factory class=\"F\" method=\"m\"/></service></services></container>";
        assert!(run_in("/project/config/services.xml", "xml", xml, Groups::all_on()).is_empty());
    }

    #[test]
    fn deprecated_factory_setting_is_quiet_in_route_files() {
        let text = "services:\n    app.a:\n        factory_method: create\n";
        assert!(run_in("/project/config/routes.yaml", "yaml", text, Groups::all_on()).is_empty());
    }

    #[test]
    fn backtick_start_is_reported_as_unquoted_scalar() {
        let found = yaml("a: `ls`\n");
        assert_eq!(codes(&found), ["unquoted-yaml-scalar"]);
        assert_eq!(found[0].range, TextRange::new(Position::new(0, 3), Position::new(0, 7)));
    }

    #[test]
    fn pipe_followed_by_text_is_reported_but_block_scalar_header_is_not() {
        assert_eq!(codes(&yaml("a: |foo\nb: >x y\n")), ["unquoted-yaml-scalar", "unquoted-yaml-scalar"]);
        assert!(yaml("a: |\n  x: y\nb: |-\n  z\nc: >2+ # note\n  w\nd: >\n").is_empty());
    }

    #[test]
    fn unescaped_backslash_in_double_quotes_is_a_hint_with_a_doubling_fix() {
        let text = "a: \"App\\Foo\"\n";
        let found = yaml(text);
        assert_eq!(codes(&found), ["yaml-unescaped-backslash"]);
        assert_eq!(found[0].range, TextRange::new(Position::new(0, 7), Position::new(0, 8)));
        assert_eq!(found[0].severity, openidea_plugin::Severity::Hint);

        let findings = crate::config_checks::yaml_scalars(SERVICES, "yaml", text);
        assert_eq!(findings[0].replacement.as_deref(), Some("\\\\"));
    }

    #[test]
    fn valid_escapes_and_single_quotes_are_quiet() {
        assert!(yaml("a: \"line\\n\\t\\\\ \\\" \\x41 \\u00e9 \\/ \\_ \\0\"\nb: 'App\\Foo'\nc: App\\Foo\n").is_empty());
        let long = format!("a: \"{}\\q\"\n", "x".repeat(300));
        assert!(yaml(&long).is_empty());
    }

    #[test]
    fn unquoted_colon_in_a_mapping_value_is_a_hint() {
        let found = yaml("a: foo: bar\n- b: x: y # c: d\n");
        assert_eq!(codes(&found), ["yaml-unquoted-colon", "yaml-unquoted-colon"]);
        assert_eq!(found[0].range, TextRange::new(Position::new(0, 3), Position::new(0, 11)));
        assert_eq!(found[1].range, TextRange::new(Position::new(1, 5), Position::new(1, 9)));
        assert_eq!(found[0].severity, openidea_plugin::Severity::Hint);
    }

    #[test]
    fn urls_times_and_quoted_values_are_quiet_for_colon_check() {
        let text = "a: http://x.test/y\nb: 12:30\nc: \"a: b\"\nd: 'a: b'\ne: [a: b]\nf: {a: b}\ng: foo \"x: y\"\nh: &anchor x: y\ni: !tag a: b\nj: *ref\n";
        assert!(yaml(text).is_empty());
    }

    #[test]
    fn colon_check_ignores_block_scalars_and_comments() {
        assert!(yaml("a: |\n  b: c: d\n# e: f: g\nh: i # j: k\n").is_empty());
    }

    #[test]
    fn yaml_group_ranges_use_utf16_columns_with_multibyte_text() {
        let found = yaml("a: \"é😀\\q\"\nb: é😀 x: y\n");
        assert_eq!(codes(&found), ["yaml-unescaped-backslash", "yaml-unquoted-colon"]);
        assert_eq!(found[0].range.start, Position::new(0, 7));
        assert_eq!(found[1].range, TextRange::new(Position::new(1, 3), Position::new(1, 11)));
    }

    #[test]
    fn yaml_group_crlf_files_report_correct_lines() {
        let found = yaml("a: b\r\nc: `x`\r\nd: \"\\q\"\r\ne: f: g\r\n");
        assert_eq!(found[0].range, TextRange::new(Position::new(1, 3), Position::new(1, 6)));
        assert_eq!(found[1].range, TextRange::new(Position::new(2, 4), Position::new(2, 5)));
        assert_eq!(found[2].range, TextRange::new(Position::new(3, 3), Position::new(3, 7)));
    }

    #[test]
    fn yaml_group_fixed_file_publishes_empty_list() {
        let host = MockHost::install();
        host.add_file("composer.json", r#"{"require": {"symfony/framework-bundle": "^7.0"}}"#);
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();

        host.add_file("config/services.yaml", "a: `x`\nb: \"\\q\"\nc: d: e\nservices:\n    s:\n        factory_method: m\n");
        plugin.handle_events(&[Event::FileOpened(SERVICES.into())], &host.invocation());
        assert_eq!(host.diagnostics()[SERVICES].len(), 4);

        host.add_file("config/services.yaml", "a: '`x`'\nb: \"\\\\q\"\nc: 'd: e'\nservices:\n    s:\n        factory: m\n");
        plugin.handle_events(&[Event::FileSaved(SERVICES.into())], &host.invocation());
        assert!(host.diagnostics().get(SERVICES).is_none_or(Vec::is_empty));
    }

    #[test]
    fn yaml_syntax_group_can_be_switched_off() {
        let text = "a: @x\nb: `y`\nc: d: e\nd: \"\\q\"\nservices:\n    s:\n        factory_method: m\n";
        let off = Groups { yaml_syntax: false, ..Groups::all_on() };
        assert_eq!(codes(&run_in(SERVICES, "yaml", text, off)), ["deprecated-factory-setting"]);

        let off = Groups { services: false, ..Groups::all_on() };
        assert!(!codes(&run_in(SERVICES, "yaml", text, off)).contains(&"deprecated-factory-setting"));
    }

    #[test]
    fn yaml_checks_ignore_files_outside_config_and_translations() {
        let text = "a: @x\nb: `y`\nc: d: e\nd: \"\\q\"\n";
        assert!(run_in("/project/docker/compose.yaml", "yaml", text, Groups::all_on()).is_empty());
        assert!(!run_in("/project/translations/messages.en.yaml", "yaml", text, Groups::all_on()).is_empty());
    }
}
