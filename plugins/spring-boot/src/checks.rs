//! Text checks over one Java test or component. A check stays quiet when an
//! annotation comes from a library other than Spring or the project's
//! configuration is not fully visible.

use openidea_plugin::{Severity, host};
use spring_text::finding::Finding;
use spring_text::java::{JavaFile, Member, MemberKind, TypeDecl};

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
const AUTOWIRE_MODE_KEY: &str = "constructor.autowire.mode";

/// Which groups of checks run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Groups {
    pub test_constructors: bool,
    pub event_listeners: bool,
}

impl Groups {
    pub fn from_settings() -> Self {
        let on = |key: &str| host::setting_bool(&format!("springBoot.{key}")).unwrap_or(true);
        Self { test_constructors: on("checkTestConstructors"), event_listeners: on("checkEventListeners") }
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
    }
    out
}

fn from_spring(file: &JavaFile, name: &str) -> bool {
    let suffix = format!(".{name}");
    file.imports
        .iter()
        .filter(|i| !i.is_static && i.path.ends_with(&suffix))
        .all(|i| i.path.starts_with("org.springframework."))
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
        let Some(name) = EVENT_LISTENERS.iter().find(|n| m.annotation(n).is_some() && from_spring(file, n)) else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use spring_text::java::parse;

    fn run(text: &str, mode_set: Option<bool>) -> Vec<(&'static str, String)> {
        let found = check_java(&parse(text), mode_set, Groups { test_constructors: true, event_listeners: true });
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
        let found = check_java(&parse(LISTENERS), None, Groups { test_constructors: true, event_listeners: true });
        assert_eq!((found[0].severity, found[1].severity), (Severity::Error, Severity::Hint));
    }

    #[test]
    fn event_listener_signature_is_quiet_for_library_or_dynamic_name() {
        let foreign = LISTENERS.replace("org.springframework.context.event.EventListener", "org.acme.EventListener");
        assert!(run(&foreign, None).is_empty());
    }
}
