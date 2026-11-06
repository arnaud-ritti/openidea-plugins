//! Text checks over one Java test or component. A check stays quiet when an
//! annotation comes from a library other than Spring or the project's
//! configuration is not fully visible.

use openidea_plugin::{Severity, host};
use spring_text::finding::Finding;
use spring_text::java::{Annotation, JavaFile, Member, MemberKind, TypeDecl};

const SPRING_TEST_ANNOTATIONS: [&str; 17] = [
    "SpringBootTest",
    "WebMvcTest",
    "WebFluxTest",
    "DataJpaTest",
    "DataJdbcTest",
    "JdbcTest",
    "DataMongoTest",
    "DataRedisTest",
    "JsonTest",
    "RestClientTest",
    "WebServiceClientTest",
    "ContextConfiguration",
    "SpringJUnitConfig",
    "SpringJUnitWebConfig",
    "TestExecutionListeners",
    "DataNeo4jTest",
    "JooqTest",
];
const JUNIT_PARAMETERS: [&str; 4] = ["TestInfo", "TestReporter", "RepetitionInfo", "CapturedOutput"];
const EVENT_LISTENERS: [&str; 2] = ["EventListener", "TransactionalEventListener"];
const PAYLOAD_RETURNS: [&str; 17] = [
    "boolean", "byte", "short", "int", "long", "float", "double", "char", "Boolean", "Byte", "Short", "Integer",
    "Long", "Float", "Double", "Character", "String",
];
const EARLY_EVENTS: [&str; 4] = [
    "ApplicationStartingEvent",
    "ApplicationEnvironmentPreparedEvent",
    "ApplicationContextInitializedEvent",
    "ApplicationPreparedEvent",
];
const REDUNDANT_ANNOTATIONS: [&str; 2] = ["EnableAutoConfiguration", "ComponentScan"];
const AUTOWIRE_MODE_KEY: &str = "constructor.autowire.mode";

/// Which groups of checks run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Groups {
    pub test_constructors: bool,
    pub event_listeners: bool,
    pub configuration_properties: bool,
    pub early_events: bool,
    pub application_setup: bool,
}

impl Groups {
    pub fn from_settings() -> Self {
        let on = |key: &str| host::setting_bool(&format!("springBoot.{key}")).unwrap_or(true);
        Self {
            test_constructors: on("checkTestConstructors"),
            event_listeners: on("checkEventListeners"),
            configuration_properties: on("checkConfigurationProperties"),
            early_events: on("checkEarlyEvents"),
            application_setup: on("checkApplicationSetup"),
        }
    }
}

/// Reads one `spring.properties`, build or configuration file: `Some` when it sets the autowire mode.
pub fn sets_autowire_mode(_path: &str, text: &str) -> Option<bool> {
    text.to_ascii_lowercase().replace('_', ".").contains(AUTOWIRE_MODE_KEY).then_some(true)
}

/// `autowire_mode_set`: `None` while the project is not fully read, else whether any file sets the mode.
pub fn check_java(file: &JavaFile, autowire_mode_set: Option<bool>, groups: Groups) -> Vec<Finding> {
    let mut out = Vec::new();
    for ty in file.types.iter().filter(|t| t.kind == "class") {
        if groups.test_constructors && autowire_mode_set == Some(false) {
            test_constructor_autowire(file, ty, &mut out);
        }
        if groups.event_listeners {
            event_listener_signature(file, ty, &mut out);
        }
        if groups.early_events {
            early_event_listeners(file, ty, &mut out);
        }
    }
    for ty in &file.types {
        if groups.configuration_properties {
            configuration_properties_prefix(file, ty, &mut out);
        }
        if groups.application_setup {
            application_setup(file, ty, &mut out);
        }
    }
    out
}

/// For each argument-less `@EnableAutoConfiguration` or `@ComponentScan` on a `@SpringBootApplication` type: the byte range to delete (the annotation and its trailing blank and line break) and the title of the fix.
pub fn removal_fixes(file: &JavaFile, text: &str) -> Vec<(usize, usize, String)> {
    file.types
        .iter()
        .flat_map(redundant_annotations(file))
        .map(|a| {
            let rest = &text[a.end..];
            let blank = rest.len() - rest.trim_start_matches([' ', '\t']).len();
            let after = &rest[blank..];
            let line_break = ["\r\n", "\n"].iter().find(|b| after.starts_with(**b)).map_or(0, |b| b.len());
            (a.at, a.end + blank + line_break, format!("Remove redundant @{}", a.name))
        })
        .collect()
}

fn redundant_annotations<'f>(file: &'f JavaFile) -> impl Fn(&'f TypeDecl) -> Vec<&'f Annotation> {
    move |ty| {
        if !ty.has(&["SpringBootApplication"]) || !file.from_spring("SpringBootApplication") {
            return Vec::new();
        }
        ty.annotations
            .iter()
            .filter(|a| REDUNDANT_ANNOTATIONS.contains(&a.name.as_str()) && a.args.is_empty() && file.from_spring(&a.name))
            .collect()
    }
}

fn is_spring_test(ty: &TypeDecl) -> bool {
    ty.has(&SPRING_TEST_ANNOTATIONS)
        || ty
            .annotation("ExtendWith")
            .is_some_and(|a| a.args.iter().any(|g| g.text.contains("SpringExtension")))
}

/// Spring test constructors are autowired only when `@Autowired` sits on the constructor or a parameter (also `@Qualifier`, `@Value`), when `@TestConstructor(autowireMode = ALL)` applies, or when `spring.test.constructor.autowire.mode=all` is set in `spring.properties` or as a system property; otherwise JUnit fails with a ParameterResolutionException.
fn test_constructor_autowire(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    if !is_spring_test(ty)
        || ty.modifiers.iter().any(|m| m == "abstract")
        || file.types.iter().any(|t| t.annotation("TestConstructor").is_some())
    {
        return;
    }
    let constructors: Vec<&Member> = ty.members.iter().filter(|m| m.kind == MemberKind::Constructor).collect();
    let [constructor] = constructors.as_slice() else { return };
    if constructor.annotation("Autowired").is_some() || constructor.annotation("Inject").is_some() {
        return;
    }
    let needs_injection = constructor.params.iter().any(|p| {
        !JUNIT_PARAMETERS.contains(&p.ty.name.as_str()) && p.annotations.is_empty()
    });
    let parameter_annotated = constructor.params.iter().any(|p| !p.annotations.is_empty());
    if !needs_injection || parameter_annotated {
        return;
    }
    out.push(Finding::new(
        constructor.at,
        constructor.at + constructor.name.len(),
        "test-constructor-autowire",
        "Spring autowires the parameters of a test constructor only when it (or a parameter) has @Autowired, when @TestConstructor(autowireMode = ALL) applies, or when spring.test.constructor.autowire.mode=all is set in spring.properties or as a system property. Otherwise JUnit fails with a ParameterResolutionException.",
    ));
}

/// An `@EventListener` method takes at most one parameter (the event, or none with `classes`); its return value is published as a new event, so a plain value such as a `String` or `int` is probably unintended.
fn event_listener_signature(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    for m in ty.methods() {
        let Some(name) = EVENT_LISTENERS.iter().find(|n| m.annotation(n).is_some() && file.from_spring(n)) else {
            continue;
        };
        let end = m.at + m.name.len();
        if m.params.len() > 1 {
            out.push(
                Finding::new(m.at, end, "event-listener-signature", format!("An @{name} method takes at most one parameter, the event; Spring throws IllegalStateException for {} parameters.", m.params.len()))
                    .with_severity(Severity::Error),
            );
            continue;
        }
        if m.ty.as_ref().is_some_and(|t| !t.array && t.args.is_empty() && PAYLOAD_RETURNS.contains(&t.name.as_str())) {
            out.push(
                Finding::new(m.at, end, "event-listener-signature", format!("The value an @{name} method returns is published as a new event; a plain `{}` is probably not meant to be one. Return void, an event object or a collection of events.", m.ty.as_ref().map(|t| t.name.as_str()).unwrap_or_default()))
                    .with_severity(Severity::Hint),
            );
        }
    }
}

fn valid_prefix(prefix: &str) -> bool {
    prefix.split('.').all(|segment| {
        segment.split('-').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
    })
}

/// A `@ConfigurationProperties` prefix is binding-safe when each dot-separated segment is lower case, kebab-separated; a missing or empty prefix is only a hint. Constants, placeholders and concatenations are not read.
fn configuration_properties_prefix(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    if !file.from_spring("ConfigurationProperties") {
        return;
    }
    let annotated = ty.annotation("ConfigurationProperties").into_iter().chain(
        ty.methods().filter(|m| m.annotation("Bean").is_some()).filter_map(|m| m.annotation("ConfigurationProperties")),
    );
    for annotation in annotated {
        let prefix = annotation.arg("prefix").or_else(|| annotation.arg("value")).filter(|p| p.text.trim() != "\"\"");
        let Some(prefix) = prefix else {
            out.push(
                Finding::new(annotation.name_at, annotation.name_at + annotation.name.len(), "config-properties-prefix", "@ConfigurationProperties has no prefix, so it binds properties at the top level. Give it a prefix such as `app.mail`.")
                    .with_severity(Severity::Hint),
            );
            continue;
        };
        let [literal] = prefix.strings.as_slice() else { continue };
        if !prefix.text.starts_with('"') || !prefix.text.ends_with('"') || prefix.text.len() != literal.end - literal.at + 2 {
            continue;
        }
        if literal.value.contains("${") || valid_prefix(&literal.value) {
            continue;
        }
        out.push(Finding::new(
            prefix.at,
            prefix.end,
            "config-properties-prefix",
            format!("`{}` is not a valid @ConfigurationProperties prefix: use lower-case words separated by `-`, joined by single dots (for example `app.mail-server`).", literal.value),
        ));
    }
}

/// The application starts the context's listeners before `@EventListener` beans exist, so a listener for one of the early events never fires.
fn early_event_listeners(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    for m in ty.methods() {
        let Some(listener) = m.annotation("EventListener") else { continue };
        if !file.from_spring("EventListener") {
            continue;
        }
        let from_parameter = match m.params.as_slice() {
            [p] => EARLY_EVENTS.iter().find(|e| p.ty.name == **e),
            _ => None,
        };
        let from_classes = listener.arg("classes").and_then(|a| {
            EARLY_EVENTS.iter().find(|e| a.text.contains(&format!("{e}.class")))
        });
        let Some(event) = from_parameter.or(from_classes) else { continue };
        if !file.from_spring(event) {
            continue;
        }
        out.push(Finding::new(
            m.at,
            m.at + m.name.len(),
            "early-event-listener",
            format!("{event} is published before the application context exists, so this @EventListener is never called. Register the listener with SpringApplication.addListeners or in META-INF/spring.factories."),
        ));
    }
}

/// A `@SpringBootApplication` type belongs in a named package, and already enables auto-configuration and component scanning.
fn application_setup(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    if !ty.has(&["SpringBootApplication"]) || !file.from_spring("SpringBootApplication") {
        return;
    }
    if file.package.is_empty() {
        out.push(Finding::new(
            ty.at,
            ty.at + ty.name.len(),
            "default-package-application",
            "A @SpringBootApplication in the default package makes component scanning cover every class on the classpath. Move it into a named package.",
        ));
    }
    for annotation in redundant_annotations(file)(ty) {
        let code = match annotation.name.as_str() {
            "EnableAutoConfiguration" => "redundant-enable-auto-configuration",
            _ => "redundant-component-scan",
        };
        out.push(Finding::new(
            annotation.at,
            annotation.end,
            code,
            format!("@SpringBootApplication already includes @{}; this annotation is redundant.", annotation.name),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spring_text::java::parse;

    const ALL: Groups = Groups {
        test_constructors: true,
        event_listeners: true,
        configuration_properties: true,
        early_events: true,
        application_setup: true,
    };

    fn run(text: &str, mode_set: Option<bool>) -> Vec<(&'static str, String)> {
        let found = check_java(&parse(text), mode_set, ALL);
        found.iter().map(|f| (f.code, text[f.start..f.end].to_owned())).collect()
    }

    const TEST: &str = "import org.springframework.boot.test.context.SpringBootTest;\n@SpringBootTest\nclass OwnerTests {\n    OwnerTests(OwnerService service) {}\n}\n";

    #[test]
    fn test_constructor_autowire_warns() {
        assert_eq!(run(TEST, Some(false)), [("test-constructor-autowire", "OwnerTests".to_owned())]);
    }

    #[test]
    fn test_constructor_autowire_is_quiet_for_library_or_dynamic_name() {
        assert!(run(TEST, None).is_empty());
        assert!(run(TEST, Some(true)).is_empty());
        let quiet = |from: &str, to: &str| run(&TEST.replace(from, to), Some(false)).is_empty();
        assert!(quiet("    OwnerTests(", "    @Autowired OwnerTests("));
        assert!(quiet("OwnerService service", "@Qualifier(\"a\") OwnerService service"));
        assert!(quiet("@SpringBootTest\n", "@SpringBootTest\n@TestConstructor(autowireMode = AutowireMode.ALL)\n"));
        assert!(quiet("OwnerService service", "TestInfo info"));
        assert!(quiet("@SpringBootTest\n", "@ExtendWith(MockitoExtension.class)\n"));
        assert!(quiet("OwnerTests(OwnerService service) {}", "OwnerTests() {}"));
        assert!(quiet("OwnerTests(OwnerService service) {}", "OwnerTests(A a) {}\n    OwnerTests(B b) {}"));
    }

    #[test]
    fn autowire_mode_is_read_from_property_or_env_style_names() {
        assert_eq!(sets_autowire_mode("spring.properties", "spring.test.constructor.autowire.mode=all"), Some(true));
        assert_eq!(sets_autowire_mode("build.gradle", "systemProperty 'spring.test.constructor.autowire.mode', 'all'"), Some(true));
        assert_eq!(sets_autowire_mode("spring.properties", "other=1"), None);
    }

    const LISTENERS: &str = "import org.springframework.context.event.EventListener;\nclass L {\n    @EventListener\n    void two(A a, B b) {}\n    @EventListener\n    String text(A a) { return \"\"; }\n    @EventListener\n    BEvent ok(A a) { return null; }\n    @EventListener\n    List<BEvent> many(A a) { return null; }\n    @EventListener(classes = A.class)\n    void none() {}\n}\n";

    #[test]
    fn constructor_param_with_foreign_annotation_is_quiet() {
        let text = TEST.replace("(OwnerService service)", "(@TempDir Path dir, OwnerService service)");
        assert!(run(&text, Some(false)).is_empty());
        let mock = TEST.replace("(OwnerService service)", "(@Mock OwnerService service)");
        assert!(run(&mock, Some(false)).is_empty());
    }

    #[test]
    fn event_listener_signature_warns() {
        assert_eq!(
            run(LISTENERS, None),
            [("event-listener-signature", "two".to_owned()), ("event-listener-signature", "text".to_owned())]
        );
        let found = check_java(&parse(LISTENERS), None, ALL);
        assert_eq!((found[0].severity, found[1].severity), (Severity::Error, Severity::Hint));
    }

    #[test]
    fn event_listener_signature_is_quiet_for_library_or_dynamic_name() {
        let foreign = LISTENERS.replace("org.springframework.context.event.EventListener", "org.acme.EventListener");
        assert!(run(&foreign, None).is_empty());
    }

    fn codes(text: &str) -> Vec<(&'static str, String)> {
        run(text, None)
    }

    fn severities(text: &str) -> Vec<Severity> {
        check_java(&parse(text), None, ALL).iter().map(|f| f.severity).collect()
    }

    const PROPS: &str = "import org.springframework.boot.context.properties.ConfigurationProperties;\n@ConfigurationProperties(prefix = \"app.mail\")\nclass MailProperties {}\n";

    #[test]
    fn uppercase_prefix_warns() {
        let text = PROPS.replace("app.mail", "app.Mail");
        assert_eq!(codes(&text), [("config-properties-prefix", "\"app.Mail\"".to_owned())]);
        assert_eq!(severities(&text), [Severity::Warning]);
    }

    #[test]
    fn underscore_prefix_warns() {
        let text = PROPS.replace("prefix = \"app.mail\"", "\"app.mail_server\"");
        assert_eq!(codes(&text).len(), 1);
        let method = "import org.springframework.boot.context.properties.ConfigurationProperties;\nclass C {\n    @Bean\n    @ConfigurationProperties(\"app..x\")\n    Props props() { return null; }\n}\n";
        assert_eq!(codes(method), [("config-properties-prefix", "\"app..x\"".to_owned())]);
    }

    #[test]
    fn missing_prefix_hints() {
        let text = PROPS.replace("(prefix = \"app.mail\")", "");
        assert_eq!(codes(&text), [("config-properties-prefix", "ConfigurationProperties".to_owned())]);
        assert_eq!(severities(&text), [Severity::Hint]);
        assert_eq!(severities(&PROPS.replace("prefix = \"app.mail\"", "ignoreUnknownFields = false")), [Severity::Hint]);
    }

    #[test]
    fn empty_prefix_hints() {
        let text = PROPS.replace("prefix = \"app.mail\"", "\"\"");
        assert_eq!(codes(&text), [("config-properties-prefix", "ConfigurationProperties".to_owned())]);
        assert_eq!(severities(&text), [Severity::Hint]);
        assert_eq!(severities(&PROPS.replace("app.mail", "")), [Severity::Hint]);
    }

    #[test]
    fn kebab_case_prefix_is_quiet() {
        assert!(codes(PROPS).is_empty());
        assert!(codes(&PROPS.replace("app.mail", "app.mail-server.v2")).is_empty());
        assert!(codes(&PROPS.replace("prefix = \"app.mail\"", "\"mail\"")).is_empty());
    }

    #[test]
    fn config_properties_prefix_is_quiet_for_library_or_dynamic_name() {
        assert!(codes(&PROPS.replace("\"app.mail\"", "PREFIX")).is_empty());
        assert!(codes(&PROPS.replace("\"app.mail\"", "\"${app.Prefix}\"")).is_empty());
        assert!(codes(&PROPS.replace("\"app.mail\"", "\"app.\" + NAME")).is_empty());
        let foreign = PROPS.replace("org.springframework.boot.context.properties", "org.acme");
        assert!(codes(&foreign.replace("app.mail", "App")).is_empty());
        assert!(codes(&foreign.replace("(prefix = \"app.mail\")", "")).is_empty());
    }

    const EARLY: &str = "import org.springframework.context.event.EventListener;\nimport org.springframework.boot.context.event.ApplicationStartingEvent;\nclass L {\n    @EventListener\n    void on(ApplicationStartingEvent e) {}\n    @EventListener(classes = ApplicationPreparedEvent.class)\n    void other() {}\n}\n";

    #[test]
    fn early_event_listener_warns_for_parameter_and_classes_argument() {
        assert_eq!(
            codes(EARLY),
            [("early-event-listener", "on".to_owned()), ("early-event-listener", "other".to_owned())]
        );
        let found = check_java(&parse(EARLY), None, ALL);
        assert!(found[0].message.contains("SpringApplication.addListeners"));
        assert!(found[0].message.contains("spring.factories"));
    }

    #[test]
    fn ready_event_listener_is_quiet() {
        let text = EARLY.replace("ApplicationStartingEvent e", "ApplicationReadyEvent e").replace("ApplicationPreparedEvent.class", "ApplicationReadyEvent.class");
        assert!(codes(&text).is_empty());
        let transactional = "import org.springframework.transaction.event.TransactionalEventListener;\nclass L {\n    @TransactionalEventListener\n    void on(OrderPlaced e) {}\n}\n";
        assert!(codes(transactional).is_empty());
    }

    #[test]
    fn early_event_listener_is_quiet_for_library_or_dynamic_name() {
        let foreign = EARLY.replace("org.springframework.boot.context.event", "org.acme");
        assert_eq!(codes(&foreign), [("early-event-listener", "other".to_owned())]);
        let foreign = foreign.replace("ApplicationPreparedEvent.class", "EVENTS");
        assert!(codes(&foreign).is_empty());
        let listener = EARLY.replace("org.springframework.context.event.EventListener", "org.acme.EventListener");
        assert!(codes(&listener).is_empty());
        assert!(codes(&EARLY.replace("ApplicationStartingEvent e", "T e").replace("ApplicationPreparedEvent.class", "TYPES")).is_empty());
    }

    const APP: &str = "package com.acme;\n\nimport org.springframework.boot.autoconfigure.SpringBootApplication;\nimport org.springframework.boot.autoconfigure.EnableAutoConfiguration;\n\n@SpringBootApplication\n@EnableAutoConfiguration\n@ComponentScan\npublic class App {}\n";

    #[test]
    fn default_package_application_warns() {
        let text = "import org.springframework.boot.autoconfigure.SpringBootApplication;\n@SpringBootApplication\nclass App {}\n";
        assert_eq!(codes(text), [("default-package-application", "App".to_owned())]);
    }

    #[test]
    fn packaged_application_is_quiet() {
        let text = APP.replace("@EnableAutoConfiguration\n@ComponentScan\n", "");
        assert!(codes(&text).is_empty());
    }

    #[test]
    fn redundant_annotations_warn_and_offer_removal() {
        assert_eq!(
            codes(APP),
            [
                ("redundant-enable-auto-configuration", "@EnableAutoConfiguration".to_owned()),
                ("redundant-component-scan", "@ComponentScan".to_owned()),
            ]
        );
        let titles: Vec<_> = removal_fixes(&parse(APP), APP).into_iter().map(|f| f.2).collect();
        assert_eq!(titles, ["Remove redundant @EnableAutoConfiguration", "Remove redundant @ComponentScan"]);
    }

    #[test]
    fn component_scan_with_arguments_is_quiet() {
        let text = APP.replace("@ComponentScan", "@ComponentScan(\"com.acme\")").replace("@EnableAutoConfiguration", "@EnableAutoConfiguration(exclude = X.class)");
        assert!(codes(&text).is_empty());
        assert!(removal_fixes(&parse(&text), &text).is_empty());
    }

    #[test]
    fn application_setup_is_quiet_for_library_or_dynamic_name() {
        let foreign = APP.replace("org.springframework.boot.autoconfigure.SpringBootApplication", "org.acme.SpringBootApplication");
        assert!(codes(&foreign).is_empty());
        let default_package = foreign.replace("package com.acme;\n", "");
        assert!(codes(&default_package).is_empty());
        let other_enable = APP.replace("org.springframework.boot.autoconfigure.EnableAutoConfiguration", "org.acme.EnableAutoConfiguration");
        assert_eq!(codes(&other_enable).len(), 1);
        assert!(codes("package a;\n@ComponentScan\n@EnableAutoConfiguration\nclass Config {}\n").is_empty());
    }

    #[test]
    fn removal_fix_deletes_the_annotation_line() {
        let fixes = removal_fixes(&parse(APP), APP);
        let (from, to, _) = &fixes[0];
        assert_eq!(&APP[*from..*to], "@EnableAutoConfiguration\n");
        let crlf = APP.replace('\n', "\r\n");
        let (from, to, _) = &removal_fixes(&parse(&crlf), &crlf)[1];
        assert_eq!(&crlf[*from..*to], "@ComponentScan\r\n");
    }

    #[test]
    fn groups_can_be_switched_off() {
        let text = format!("{}{EARLY}{APP}", PROPS.replace("app.mail", "App"));
        assert_eq!(check_java(&parse(&text), None, ALL).len(), 5);
        let off = |g: Groups| check_java(&parse(&text), None, g).len();
        assert_eq!(off(Groups { configuration_properties: false, ..ALL }), 4);
        assert_eq!(off(Groups { early_events: false, ..ALL }), 3);
        assert_eq!(off(Groups { application_setup: false, ..ALL }), 3);
    }

    #[test]
    fn ranges_use_utf16_columns_with_multibyte_text() {
        let text = PROPS.replace("app.mail", "app.Mailé😀");
        let d = check_java(&parse(&text), None, ALL)[0].diagnostic(&text);
        assert_eq!((d.range.start.line, d.range.start.character), (1, 34));
        assert_eq!(d.range.end.character - d.range.start.character, 13);
    }

    #[test]
    fn crlf_files_report_correct_lines() {
        let text = APP.replace('\n', "\r\n");
        let lines: Vec<_> = check_java(&parse(&text), None, ALL).iter().map(|f| f.diagnostic(&text).range.start.line).collect();
        assert_eq!(lines, [6, 7]);
    }
}
