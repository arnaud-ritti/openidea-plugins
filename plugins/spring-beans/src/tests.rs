use super::*;
use openidea_plugin::Event;
use openidea_plugin::mock::MockHost;
use spring_text::xml;
use crate::annotation_checks;

const SERVICE: &str = r#"package com.acme;

import org.springframework.stereotype.Service;

/** A @Component in a comment is not a bean. */
@Service("users")
@Profile({"dev", "prod"})
public class UserServiceImpl implements UserService {
    private final UserRepository repo;
    private final List<Notifier> notifiers;

    public UserServiceImpl(UserRepository repo, @Qualifier("sms") Notifier main, List<Notifier> notifiers) {
        this.repo = repo;
        this.notifiers = notifiers;
    }
}
"#;

const CONTROLLER: &str = r##"package com.acme;

@RestController
class UserController {
    @Autowired
    private UserService users;

    @Resource(name = "clock") private Clock clock;

    @Value("${app.name}") private String name;

    private int count = 0;

    @Cacheable(value = "users", key = "#id + #r", unless = "#result == null")
    User find(long id, String region) { return users.find(id); }

    @PreAuthorize("@guard.allows(#id)")
    void delete(long id) {}
}
"##;

const CONFIG: &str = r#"package com.acme;

@Configuration
public class AppConfig {
    @Bean
    public Clock clock() { return Clock.systemUTC(); }

    @Bean(name = {"mail", "mailer"})
    @Primary
    MailNotifier mailNotifier(Clock clock) { return new MailNotifier(clock); }

    @Bean
    Guard guard() { return new Guard(); }
}

interface UserRepository extends JpaRepository<User, Long> {}

class MailNotifier extends AbstractNotifier {}

abstract class AbstractNotifier implements Notifier {}

@Component("sms")
@RequiredArgsConstructor
class SmsNotifier implements Notifier {
    private final Clock clock;
    private static final int RETRIES = 3;
}
"#;

const ASPECT: &str = r#"package com.acme;

@Aspect
@Component
class Auditing {
    @Pointcut("within(com.acme..*)")
    void inApp() {}

    @Before("inApp() && bean(users) && execution(* *(..))")
    void audit() {}
}
"#;

const XML: &str = r#"<?xml version="1.0"?>
<beans xmlns="http://www.springframework.org/schema/beans">
  <bean id="legacy" class="com.acme.SmsNotifier" depends-on="clock, mail">
    <property name="clock" ref="clock"/>
  </bean>
  <alias name="legacy" alias="old"/>
  <beans profile="test">
    <bean name="fake" class="com.acme.MailNotifier"/>
  </beans>
</beans>
"#;

const KOTLIN: &str = r#"package com.acme.kt

@Service
class ReportService(
    private val repo: UserRepository,
    @Qualifier("sms") private val notifier: Notifier,
) : Reports, AutoCloseable {
    @Autowired
    lateinit var clock: Clock

    override fun close() {
        val x = object : Runnable { override fun run() {} }
    }
}

@Configuration
class KtConfig {
    @Bean
    fun reports(service: ReportService): ReportFacade = ReportFacade(service)
}
"#;

const ROOT: &str = "/project/src/main/java/com/acme";

fn setup() -> MockHost {
    let host = MockHost::install();
    host.add_file("src/main/java/com/acme/UserServiceImpl.java", SERVICE);
    host.add_file("src/main/java/com/acme/UserController.java", CONTROLLER);
    host.add_file("src/main/java/com/acme/AppConfig.java", CONFIG);
    host.add_file("src/main/java/com/acme/Auditing.java", ASPECT);
    host.add_file("src/main/resources/beans.xml", XML);
    host.add_file("src/main/kotlin/com/acme/kt/Reports.kt", KOTLIN);
    host.add_file("pom.xml", "<project/>");
    host
}

fn doc(path: &str, text: &str) -> Document {
    let language = match path.rsplit('.').next() {
        Some("kt") => "kotlin",
        Some("xml") => "xml",
        _ => "java",
    };
    Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
}

fn java(name: &str, text: &str) -> Document {
    doc(&format!("{ROOT}/{name}"), text)
}

/// The position just after the `n`th occurrence of `needle` (1-based).
fn after(text: &str, needle: &str, n: usize) -> Position {
    let at = text.match_indices(needle).nth(n - 1).unwrap().0 + needle.len();
    position(text, at)
}

fn names(model: &FileModel) -> Vec<(&str, &str)> {
    model.beans.iter().map(|b| (b.kind.as_str(), b.name.as_str())).collect()
}

fn injected(model: &FileModel) -> Vec<(&str, Option<&str>, bool)> {
    model.injections.iter().map(|i| (i.ty.as_str(), i.qualifier.as_deref(), i.multiple)).collect()
}

#[test]
fn reads_java_beans_and_injection_points() {
    let service = parse("A.java", SERVICE).unwrap();
    assert_eq!(names(&service), [("@Service", "users")]);
    assert_eq!(service.beans[0].profiles, ["dev", "prod"]);
    assert_eq!(
        injected(&service),
        [("UserRepository", None, false), ("Notifier", Some("sms"), false), ("Notifier", None, true)]
    );

    let controller = parse("A.java", CONTROLLER).unwrap();
    assert_eq!(injected(&controller), [("UserService", None, false), ("Clock", Some("clock"), false)], "@Value is config");
    assert!(controller.injections[0].field_annotation.is_some());
    assert_eq!(controller.injections[1].field_annotation, None, "@Resource is not flagged");

    let config = parse("A.java", CONFIG).unwrap();
    assert_eq!(
        names(&config),
        [
            ("@Configuration", "appConfig"),
            ("@Bean", "clock"),
            ("@Bean", "mail"),
            ("@Bean", "guard"),
            ("Spring Data repository", "userRepository"),
            ("@Component", "sms")
        ]
    );
    assert_eq!((config.beans[2].aliases.as_slice(), config.beans[2].primary), (&["mailer".to_owned()][..], true));
    assert_eq!(injected(&config), [("Clock", None, false), ("Clock", None, false)], "@Bean parameter and Lombok field");
    assert!(parse("A.java", "class Plain {}").unwrap().beans.is_empty());
}

#[test]
fn reads_xml_beans_and_references() {
    let xml = parse("beans.xml", XML).unwrap();
    assert_eq!(names(&xml), [("<bean>", "legacy"), ("<bean>", "fake")]);
    assert_eq!(xml.beans[0].aliases, ["old"]);
    assert_eq!((xml.beans[0].ty.as_str(), xml.beans[1].profiles.as_slice()), ("SmsNotifier", &["test".to_owned()][..]));
    let refs: Vec<_> = xml.injections.iter().map(|i| (i.name.as_str(), i.owner)).collect();
    assert_eq!(refs, [("clock", Some(0)), ("mail", Some(0)), ("clock", Some(0))]);
    assert_eq!(parse("pom.xml", "<project><bean id=\"x\"/></project>"), None);
}

#[test]
fn reads_kotlin_beans_and_injection_points() {
    let kt = parse("Reports.kt", KOTLIN).unwrap();
    assert_eq!(names(&kt), [("@Service", "reportService"), ("@Configuration", "ktConfig"), ("@Bean", "reports")]);
    assert_eq!(
        injected(&kt),
        [
            ("UserRepository", None, false),
            ("Notifier", Some("sms"), false),
            ("Clock", None, false),
            ("ReportService", None, false)
        ]
    );
    assert!(kt.injections[2].field_annotation.is_some());
    assert_eq!(kt.types[0].supers, ["Reports", "AutoCloseable"]);
    assert_eq!(kt.types[0].qualified, "com.acme.kt.ReportService");
    assert_eq!(kt.beans[2].ty, "ReportFacade");
}

#[test]
fn resolves_like_spring() {
    let _host = setup();
    let mut n = Navigator::default();
    let service = java("UserServiceImpl.java", SERVICE);
    let mut at = |needle: &str, k: usize| n_definition(&mut n, &service, after(SERVICE, needle, k));
    assert_eq!(at("(UserRepo", 1), [("AppConfig.java", 15)]);
    assert_eq!(at("@Qualifier(\"sms\") Noti", 1), [("AppConfig.java", 23)], "qualifier picks sms");
    let mut all = at("List<Noti", 2);
    all.sort();
    assert_eq!(all, [("AppConfig.java", 9), ("AppConfig.java", 23), ("beans.xml", 2), ("beans.xml", 7)], "every Notifier, through the hierarchy");

    let controller = java("UserController.java", CONTROLLER);
    let markers = n.line_markers(&controller);
    let tips: Vec<_> = markers.iter().map(|m| (m.line, m.tooltip.as_str())).collect();
    assert_eq!(tips, [(5, "Injected bean: users (@Service) [dev, prod]"), (7, "Injected bean: clock() (@Bean)")]);
}

fn n_definition(n: &mut Navigator, doc: &Document, at: Position) -> Vec<(&'static str, u32)> {
    n.definition(doc, at)
        .into_iter()
        .map(|l| (file_name(&l.path).to_owned().leak() as &'static str, l.range.start.line))
        .collect()
}

#[test]
fn primary_wins_between_candidates() {
    let _host = setup();
    let mut n = Navigator::default();
    let text = "package com.acme;\n@Service\nclass Alerts {\n    Alerts(Notifier notifier) {}\n}\n";
    let found = n_definition(&mut n, &java("Alerts.java", text), after(text, "(Noti", 1));
    assert_eq!(found, [("AppConfig.java", 9)], "the @Primary mail bean");
}

#[test]
fn beans_show_where_they_are_injected() {
    let _host = setup();
    let mut n = Navigator::default();
    let markers = n.line_markers(&java("AppConfig.java", CONFIG));
    let clock = markers.iter().find(|m| m.line == 5 && m.icon.as_deref() == Some("implemented")).unwrap();
    assert_eq!(clock.tooltip, "Injected into legacy, mail, reportService, sms, userController");
}

#[test]
fn edits_to_the_open_file_update_the_index() {
    let _host = setup();
    let mut n = Navigator::default();
    n.line_markers(&java("UserServiceImpl.java", &SERVICE.replace("@Service(\"users\")", "")));
    assert!(n.line_markers(&java("UserController.java", CONTROLLER)).iter().all(|m| m.line != 5));
}

#[test]
fn xml_references_navigate_and_complete() {
    let _host = setup();
    let mut n = Navigator::default();
    let xml = doc("/project/src/main/resources/beans.xml", XML);
    assert_eq!(n_definition(&mut n, &xml, after(XML, "ref=\"clo", 1)), [("AppConfig.java", 5)]);
    assert_eq!(n_definition(&mut n, &xml, after(XML, "clock, ma", 1)), [("AppConfig.java", 9)], "a list item");
    assert_eq!(n_definition(&mut n, &xml, after(XML, "com.acme.Sms", 1)), [("AppConfig.java", 23)], "class");

    let edited = XML.replace("ref=\"clock\"", "ref=\"\"");
    let items = n.completion(&doc("/project/src/main/resources/beans.xml", &edited), after(&edited, "ref=\"", 1), None);
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"clock") && labels.contains(&"mailer") && labels.contains(&"old"), "{labels:?}");

    let edited = XML.replace("class=\"com.acme.MailNotifier\"", "class=\"com.acme.Ma\"");
    let items = n.completion(&doc("/project/src/main/resources/beans.xml", &edited), after(&edited, "com.acme.Ma", 1), None);
    assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["com.acme.MailNotifier"]);
}

#[test]
fn spel_completes_beans_and_variables() {
    let _host = setup();
    let mut n = Navigator::default();
    let controller = java("UserController.java", CONTROLLER);
    assert_eq!(n_definition(&mut n, &controller, after(CONTROLLER, "@gua", 1)), [("AppConfig.java", 12)]);

    let text = CONTROLLER.replace("#id + #r", "#id + #");
    let items = n.completion(&java("UserController.java", &text), after(&text, "#id + #", 1), None);
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["id", "region", "root", "result"]);

    let text = CONTROLLER.replace("@guard", "@");
    let items = n.completion(&java("UserController.java", &text), after(&text, "(\"@", 1), None);
    assert!(items.iter().any(|i| i.label == "guard"));
}

#[test]
fn pointcuts_complete_and_navigate() {
    let _host = setup();
    let mut n = Navigator::default();
    let aspect = java("Auditing.java", ASPECT);
    assert_eq!(n_definition(&mut n, &aspect, after(ASPECT, "\"inAp", 1)), [("Auditing.java", 6)]);
    assert_eq!(n_definition(&mut n, &aspect, after(ASPECT, "bean(use", 1)), [("UserServiceImpl.java", 7)]);
    assert!(n_definition(&mut n, &aspect, after(ASPECT, "execu", 1)).is_empty());

    let text = ASPECT.replace("inApp() && bean", "in && bean");
    let items = n.completion(&java("Auditing.java", &text), after(&text, "(\"in", 1), None);
    let inserts: Vec<_> = items.iter().map(|i| (i.label.as_str(), i.insert_text.as_str())).collect();
    assert!(inserts.contains(&("execution", "execution(${1:* com.example..*.*(..)})")));
    assert!(inserts.contains(&("inApp", "inApp()")));
    assert!(n.completion(&aspect, after(ASPECT, "within(com.ac", 1), None).is_empty(), "designator arguments");
}

#[test]
fn kotlin_injections_navigate() {
    let _host = setup();
    let mut n = Navigator::default();
    let kt = doc("/project/src/main/kotlin/com/acme/kt/Reports.kt", KOTLIN);
    assert_eq!(n_definition(&mut n, &kt, after(KOTLIN, "val repo: UserRepo", 1)), [("AppConfig.java", 15)]);
    assert_eq!(n_definition(&mut n, &kt, after(KOTLIN, "(service: Report", 1)), [("Reports.kt", 3)]);
}

#[test]
fn flags_field_injection_and_fixes_it() {
    let host = setup();
    let mut n = Navigator::default();
    let controller = java("UserController.java", CONTROLLER);
    n.line_markers(&controller);
    let problems = &host.diagnostics()[&controller.path];
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].code.as_deref(), Some("field-injection"));

    let actions = n.code_actions(&controller, problems[0].range);
    assert_eq!(actions[0].title, "Use constructor injection");
    let mut edits = actions[0].edits[0].1.clone();
    edits.sort_by_key(|e| std::cmp::Reverse(offset(CONTROLLER, e.range.start)));
    let mut fixed = CONTROLLER.to_owned();
    for e in edits {
        let (s, t) = (offset(&fixed, e.range.start).unwrap(), offset(&fixed, e.range.end).unwrap());
        fixed.replace_range(s..t, &e.text);
    }
    assert!(fixed.contains("    private final UserService users;\n"), "{fixed}");
    assert!(fixed.contains("    private int count = 0;\n\n    UserController(UserService users) {\n        this.users = users;\n    }\n"), "{fixed}");
    assert!(n.code_actions(&java("AppConfig.java", CONFIG), Range::new(Position::new(5, 0), Position::new(5, 0))).is_empty());
}

#[test]
fn views_list_and_draw_beans() {
    let host = setup();
    let mut n = Navigator::default();
    let tree = n.render(BEANS_VIEW, None, &host.invocation()).unwrap();
    let Body::Tree(items) = tree.body else { panic!("tree expected") };
    let groups: Vec<_> = items.iter().filter(|i| i.parent.is_none()).map(|i| i.label.as_str()).collect();
    assert_eq!(
        groups,
        ["<bean>", "@Bean", "@Component", "@Configuration", "@RestController", "@Service", "Spring Data repository"]
    );
    let icon = |label: &str| items.iter().find(|i| i.label == label).and_then(|i| i.icon.as_deref());
    assert_eq!(
        [icon("<bean>"), icon("@Service"), icon("Spring Data repository")],
        [Some("file-xml"), Some("annotation"), Some("database")]
    );
    assert_eq!([icon("clock"), icon("userRepository")], [Some("method"), Some("interface")]);
    assert_eq!(tree.status.as_deref(), Some(&*format!("{} beans", items.len() - groups.len())));
    let users = items.iter().find(|i| i.label == "users").unwrap();
    assert_eq!(users.detail.as_deref(), Some("UserServiceImpl.java · dev, prod"));
    assert_eq!(icon("users"), Some("class"));
    n.handle(BEANS_VIEW, ViewEvent::Activate(users.key.clone()), &host.invocation()).unwrap();
    assert_eq!(host.opened_locations(), [(format!("{ROOT}/UserServiceImpl.java"), 7, 13)]);

    let graph = n.render(GRAPH_VIEW, None, &host.invocation()).unwrap();
    let Body::Html(page) = graph.body else { panic!("html expected") };
    assert!(page.contains(">users<") && page.contains("<path class=\"e\""));
    assert!(page.contains("<title>users (@Service)</title>"));
    let status = graph.status.unwrap();
    assert!(status.ends_with("dependencies · arrows point to dependencies"), "{status}");
    n.handle(GRAPH_VIEW, ViewEvent::Message(users.key.clone()), &host.invocation()).unwrap();
    assert_eq!(host.opened_locations().len(), 2);
    assert!(host.refreshed_views().is_empty(), "a complete scan doesn't re-render");

    n.handle(GRAPH_VIEW, ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
    assert_eq!(host.refreshed_views(), [BEANS_VIEW, GRAPH_VIEW]);
}

#[test]
fn graph_says_when_it_is_capped() {
    let host = MockHost::install();
    let beans: String = (0..MAX_GRAPH + 1).map(|i| format!("    @Bean Clock clock{i}() {{ return null; }}\n")).collect();
    host.add_file("src/main/java/com/acme/Many.java", &format!("@Configuration\nclass Many {{\n{beans}}}\n"));
    let graph = Navigator::default().render(GRAPH_VIEW, None, &host.invocation()).unwrap();
    let status = graph.status.unwrap();
    assert!(status.starts_with(&format!("{MAX_GRAPH} of {} beans shown · ", MAX_GRAPH + 2)), "{status}");
}

#[test]
fn p_and_c_namespace_refs_are_followed() {
    let host = MockHost::install();
    host.add_file("src/main/java/com/acme/AppConfig.java", CONFIG);
    let text = r#"<beans xmlns="http://www.springframework.org/schema/beans"
       xmlns:p="http://www.springframework.org/schema/p"
       xmlns:c="http://www.springframework.org/schema/c"
       xmlns:x="http://example.com/x">
  <bean id="a" class="x.A" p:dao-ref="clock" c:repo-ref="guard" c:_0-ref="clock" p:name="literal" x:thing-ref="guard"/>
</beans>
"#;
    let model = parse("beans.xml", text).unwrap();
    let refs: Vec<_> = model.injections.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(refs, ["clock", "guard", "clock"]);

    let mut n = Navigator::default();
    let xml = doc("/project/src/main/resources/beans.xml", text);
    assert_eq!(n_definition(&mut n, &xml, after(text, "dao-ref=\"clo", 1)), [("AppConfig.java", 5)]);
    assert_eq!(n_definition(&mut n, &xml, after(text, "repo-ref=\"gua", 1)), [("AppConfig.java", 12)]);
    assert_eq!(n_definition(&mut n, &xml, after(text, "_0-ref=\"clo", 1)), [("AppConfig.java", 5)]);
    assert!(n_definition(&mut n, &xml, after(text, "p:name=\"lit", 1)).is_empty());
    assert!(n_definition(&mut n, &xml, after(text, "x:thing-ref=\"gua", 1)).is_empty());
}

#[test]
fn kotlin_bean_fun_without_return_type_uses_expression_type_when_obvious() {
    let text = r#"package com.acme.kt

@Configuration
class C {
    @Bean fun a() = Foo()
    @Bean fun b(x: X) = FooImpl(x, 1)
    @Bean fun c() = someCall()
    @Bean fun d() = foo.bar()
    @Bean fun e() = Foo().with(1)
    @Bean fun f() = if (x) A() else B()
    @Bean fun g() = com.acme.Foo()
    @Bean fun h(): Typed = Other()
}
"#;
    let model = parse("C.kt", text).unwrap();
    let types: Vec<_> = model.beans.iter().skip(1).map(|b| (b.name.as_str(), b.ty.as_str())).collect();
    assert_eq!(types, [("a", "Foo"), ("b", "FooImpl"), ("c", ""), ("d", ""), ("e", ""), ("f", ""), ("g", ""), ("h", "Typed")]);
}

fn apply(text: &str, mut edits: Vec<TextEdit>) -> String {
    edits.sort_by_key(|e| std::cmp::Reverse(offset(text, e.range.start)));
    let mut fixed = text.to_owned();
    for e in edits {
        let (s, t) = (offset(&fixed, e.range.start).unwrap(), offset(&fixed, e.range.end).unwrap());
        fixed.replace_range(s..t, &e.text);
    }
    fixed
}

#[test]
fn use_constructor_injection_merges_into_existing_constructor() {
    let class = |members: &str| format!("package com.acme;\n\n@Service\nclass S {{\n    @Autowired\n    private Foo foo;\n\n{members}}}\n");
    let fix = |text: &str| constructor_injection(text, 4).map(|e| apply(text, e));

    let text = class("    private int n;\n\n    S(Bar bar) {\n        this.bar = bar;\n    }\n");
    let fixed = fix(&text).unwrap();
    assert!(fixed.contains("    private final Foo foo;\n"), "{fixed}");
    assert!(fixed.contains("    S(Bar bar, Foo foo) {\n        this.bar = bar;\n        this.foo = foo;\n    }\n"), "{fixed}");

    let fixed = fix(&class("    S() {}\n")).unwrap();
    assert!(fixed.contains("    S(Foo foo) {\n        this.foo = foo;\n    }\n"), "{fixed}");

    assert_eq!(fix(&class("    S() {}\n    S(int a) {}\n")), None, "several constructors");
    assert_eq!(fix(&class("    S() {\n        if (x) {\n    }\n")), None, "braces mismatch");
    assert_eq!(fix(&class("    S() {\n        this.foo = new Foo();\n    }\n")), None, "already assigned");
    assert_eq!(fix(&class("    S() {}\n    @Autowired void setBar(Bar bar) {}\n")), None, "autowired setter");
}

#[test]
fn merge_into_multiline_parameter_list() {
    let class = |ctor: &str| format!("package com.acme;\n\n@Service\nclass S {{\n    @Autowired\n    private Foo foo;\n\n{ctor}}}\n");
    let fix = |text: &str| constructor_injection(text, 4).map(|e| apply(text, e));

    let fixed = fix(&class("    S(\n        Bar bar\n    ) {\n        this.bar = bar;\n    }\n")).unwrap();
    assert!(fixed.contains("    S(\n        Bar bar,\n        Foo foo\n    ) {\n        this.bar = bar;\n        this.foo = foo;\n    }\n"), "{fixed}");

    let fixed = fix(&class("    S(Bar bar) {\n        super(bar);\n    }\n")).unwrap();
    assert!(fixed.contains("    S(Bar bar, Foo foo) {\n        super(bar);\n        this.foo = foo;\n    }\n"), "{fixed}");

    assert_eq!(fix(&class("    S(Foo foo) {\n        this.foo = foo;\n    }\n")), None, "parameter name clash");
    assert_eq!(fix(&class("    S() {\n        foo = new Foo();\n    }\n")), None, "bare assignment");

    let text = "package com.acme;\n\n@Service\nclass S {\n    @Autowired\n    private final Foo foo;\n\n    S() {}\n}\n";
    let fixed = fix(text).unwrap();
    assert!(fixed.contains("    private final Foo foo;\n") && !fixed.contains("final final"), "{fixed}");
    assert!(fixed.contains("    S(Foo foo) {\n        this.foo = foo;\n    }\n"), "{fixed}");
}

#[test]
fn kotlin_string_body_is_not_a_constructor_call() {
    let text = "@Configuration\nclass C {\n    @Bean fun a() = \"Foo()\"\n}\n";
    let model = parse("C.kt", text).unwrap();
    assert_eq!(model.beans[1].ty, "");
}

#[test]
fn merge_after_a_trailing_line_comment() {
    let class = |ctor: &str| format!("package com.acme;\n\n@Service\nclass S {{\n    @Autowired\n    private Foo foo;\n\n{ctor}}}\n");
    let fix = |text: &str| constructor_injection(text, 4).map(|e| apply(text, e));

    let fixed = fix(&class("    S(\n        Bar bar // the bar\n    ) {\n        this.bar = bar;\n    }\n")).unwrap();
    assert!(fixed.contains("    S(\n        Bar bar, // the bar\n        Foo foo\n    ) {\n"), "{fixed}");

    let fixed = fix(&class("    S(\n        Bar bar /* the bar */\n    ) {\n        this.bar = bar;\n    }\n")).unwrap();
    assert!(fixed.contains("    S(\n        Bar bar, /* the bar */\n        Foo foo\n    ) {\n"), "{fixed}");
}

#[test]
fn parameter_name_clash_is_refused_by_the_clash_check() {
    let text = "package com.acme;\n\n@Service\nclass S {\n    @Autowired\n    private Foo foo;\n\n    S(Foo foo) {}\n}\n";
    assert_eq!(constructor_injection(text, 4), None);
}

fn findings(host: &MockHost, n: &mut Navigator, name: &str, text: &str) -> Vec<(String, String)> {
    let document = java(name, text);
    n.line_markers(&document);
    host.diagnostics()
        .get(&document.path)
        .into_iter()
        .flatten()
        .filter(|d| d.code.as_deref() != Some("field-injection"))
        .map(|d| {
            let (from, to) = (offset(text, d.range.start).unwrap(), offset(text, d.range.end).unwrap());
            (d.code.clone().unwrap(), text[from..to].to_owned())
        })
        .collect()
}

fn found(text: &str) -> Vec<(String, String)> {
    let host = MockHost::install();
    findings(&host, &mut Navigator::default(), "Subject.java", text)
}

fn pair(code: &str, text: &str) -> (String, String) {
    (code.into(), text.into())
}

#[test]
fn transactional_on_private_method_warns() {
    let text = "@Service\nclass A {\n    @Transactional\n    private void save() {}\n}\n";
    assert_eq!(found(text), [pair("non-proxyable-method", "save")]);
}

#[test]
fn async_on_final_method_warns() {
    let text = "@Service\nclass A {\n    @Async\n    public final void send() {}\n}\n";
    assert_eq!(found(text), [pair("non-proxyable-method", "send")]);
}

#[test]
fn cacheable_on_static_method_warns() {
    let text = "@Component\nclass A {\n    @Cacheable(\"x\")\n    public static String load() { return \"\"; }\n}\n";
    assert_eq!(found(text), [pair("non-proxyable-method", "load")]);
}

#[test]
fn transactional_on_public_method_is_quiet() {
    let text = "@Service\nclass A {\n    @Transactional\n    public void save() {}\n    @Transactional\n    void other() {}\n}\n";
    assert!(found(text).is_empty());
}

#[test]
fn non_proxyable_method_is_quiet_for_library_or_dynamic_name() {
    let library = "import com.acme.Async;\n@Service\nclass A {\n    @Async\n    private void send() {}\n}\n";
    assert!(found(library).is_empty());
    let spring = "import org.springframework.scheduling.annotation.Async;\n@Service\nclass A {\n    @Async\n    private void send() {}\n}\n";
    assert_eq!(found(spring), [pair("non-proxyable-method", "send")]);
    let jakarta = "import jakarta.transaction.Transactional;\n@Service\nclass A {\n    @Transactional\n    private void send() {}\n}\n";
    assert_eq!(found(jakarta), [pair("non-proxyable-method", "send")]);
}

#[test]
fn scheduled_with_parameters_warns() {
    let text = "@Component\nclass A {\n    @Scheduled(fixedRate = 1000)\n    void tick(String now) {}\n}\n";
    assert_eq!(found(text), [pair("scheduled-signature", "tick")]);
}

#[test]
fn scheduled_without_parameters_is_quiet() {
    let text = "@Component\nclass A {\n    @Scheduled(fixedRate = 1000)\n    void tick() {}\n}\n";
    assert!(found(text).is_empty());
}

#[test]
fn scheduled_is_quiet_for_library_or_dynamic_name() {
    let text = "import org.quartz.Scheduled;\n@Component\nclass A {\n    @Scheduled(fixedRate = 1000)\n    void tick(String now) {}\n}\n";
    assert!(found(text).is_empty());
}

const DUPLICATE: &str = "@Service(\"dispatcher\")\nclass SmtpMailer {}\n";

fn with_other(other: &str) -> (MockHost, Navigator) {
    let host = setup();
    host.add_file("src/main/java/com/acme/Other.java", other);
    (host, Navigator::default())
}

#[test]
fn duplicate_bean_names_across_files_warn_on_both() {
    let other = "@Component(\"dispatcher\")\nclass Other {}\n";
    let (host, mut n) = with_other(other);
    host.add_file("src/main/java/com/acme/Subject.java", DUPLICATE);
    assert_eq!(findings(&host, &mut n, "Subject.java", DUPLICATE), [pair("duplicate-bean-name", "SmtpMailer")]);
    assert_eq!(findings(&host, &mut n, "Other.java", other), [pair("duplicate-bean-name", "Other")]);
}

#[test]
fn same_name_in_different_profiles_is_quiet() {
    let (host, mut n) = with_other("@Profile(\"prod\")\n@Component(\"dispatcher\")\nclass Other {}\n");
    let dev = "@Profile(\"dev\")\n@Service(\"dispatcher\")\nclass SmtpMailer {}\n";
    assert!(findings(&host, &mut n, "Subject.java", dev).is_empty());
    assert_eq!(findings(&host, &mut n, "Subject.java", DUPLICATE).len(), 1, "an unprofiled bean overlaps every profile");
}

#[test]
fn duplicate_bean_name_is_quiet_for_library_or_dynamic_name() {
    let (host, mut n) = with_other("@Component(\"dispatcher\")\nclass Other {}\n");
    let dynamic = "@Service(Names.DISPATCHER)\nclass Mailer {}\n";
    assert!(findings(&host, &mut n, "Subject.java", dynamic).is_empty());
    let conditional = "@Service(\"dispatcher\")\n@ConditionalOnMissingBean\nclass SmtpMailer {}\n";
    assert!(findings(&host, &mut n, "Subject.java", conditional).is_empty());
    let library = "@Service(\"mailSender\")\nclass Sender {}\n";
    assert!(findings(&host, &mut n, "Subject.java", library).is_empty());
}

#[test]
fn conditional_bean_clash_is_quiet_on_either_side() {
    let conditional = "@Configuration(proxyBeanMethods = false)\nclass Other {\n    @Bean @ConditionalOnMissingBean\n    Stopwatch stopwatch() { return null; }\n}\n";
    let plain = "@Configuration(proxyBeanMethods = false)\nclass Subject {\n    @Bean\n    Stopwatch stopwatch() { return null; }\n}\n";
    let (host, mut n) = with_other(conditional);
    host.add_file("src/main/java/com/acme/Subject.java", plain);
    assert!(findings(&host, &mut n, "Subject.java", plain).is_empty(), "the other side is conditional");
    assert!(findings(&host, &mut n, "Other.java", conditional).is_empty(), "this side is conditional");
    let (host, mut n) = with_other(plain.replace("Subject", "Other").as_str());
    assert_eq!(findings(&host, &mut n, "Subject.java", plain), [pair("duplicate-bean-name", "stopwatch")]);
}

#[test]
fn activation_succeeds_when_file_events_are_denied() {
    let host = MockHost::install().with_capabilities(&["project.read"]);
    assert_eq!(Navigator::default().activate(&host.context("aritti.spring-beans")), Ok(()));
    assert!(host.logs().iter().any(|(level, _)| *level == LogLevel::Warn));
}

#[test]
fn overloaded_bean_methods_are_not_duplicates() {
    let text = "@Configuration(proxyBeanMethods = false)\nclass C {\n    @Bean Clock clock() { return null; }\n    @Bean Clock clock(Zone zone) { return null; }\n}\n";
    assert!(found(text).is_empty());
}

#[test]
fn configuration_with_final_bean_method_warns() {
    let text = "@Configuration\nclass C {\n    @Bean\n    public final Clock clock() { return null; }\n}\n";
    assert_eq!(found(text), [pair("configuration-proxy-method", "clock")]);
    let private = "@Configuration(proxyBeanMethods = true)\nclass C {\n    @Bean\n    private Clock clock() { return null; }\n}\n";
    assert_eq!(found(private), [pair("configuration-proxy-method", "clock")]);
}

#[test]
fn proxy_bean_methods_false_is_quiet() {
    let text = "@Configuration(proxyBeanMethods = false)\nclass C {\n    @Bean\n    public final Clock clock() { return null; }\n}\n";
    assert!(found(text).is_empty());
    let lite = "@Component\nclass C {\n    @Bean\n    public final Clock clock() { return null; }\n}\n";
    assert!(found(lite).is_empty());
}

#[test]
fn configuration_proxy_method_is_quiet_for_library_or_dynamic_name() {
    let dynamic = "@Configuration(proxyBeanMethods = Settings.PROXY)\nclass C {\n    @Bean\n    public final Clock clock() { return null; }\n}\n";
    assert!(found(dynamic).is_empty());
    let library = "import com.acme.Configuration;\n@Configuration\nclass C {\n    @Bean\n    public final Clock clock() { return null; }\n}\n";
    assert!(found(library).is_empty());
    let static_bean = "@Configuration\nclass C {\n    @Bean\n    public static Clock clock() { return null; }\n}\n";
    assert!(found(static_bean).is_empty());
}

#[test]
fn capitalised_component_name_gets_a_hint() {
    let text = "@Service(\"UserService\")\nclass UserServiceImpl {}\n";
    assert_eq!(found(text), [pair("bean-name-convention", "UserService")]);
    let host = MockHost::install();
    let document = java("Subject.java", text);
    Navigator::default().line_markers(&document);
    assert_eq!(host.diagnostics()[&document.path][0].severity, Severity::Hint);
    let method = "@Configuration(proxyBeanMethods = false)\nclass C {\n    @Bean(\"Clock\")\n    Clock clock() { return null; }\n}\n";
    assert_eq!(found(method), [pair("bean-name-convention", "Clock")]);
}

#[test]
fn bean_name_convention_is_quiet_for_library_or_dynamic_name() {
    assert!(found("@Service(\"userService\")\nclass A {}\n").is_empty());
    assert!(found("@Service(\"URLMapper\")\nclass A {}\n").is_empty(), "two capitals is Spring's own default");
    assert!(found("@Service(Names.User)\nclass A {}\n").is_empty());
    assert!(found("import com.acme.Service;\n@Service(\"User\")\nclass A {}\n").is_empty());
}

const SELF_CALL: &str = "@Service\nclass A {\n    @Transactional\n    public void save() {}\n    public void run() {\n        save();\n        this.save();\n    }\n}\n";

fn self_invocation(host: &MockHost, text: &str) -> Vec<(String, String)> {
    findings(host, &mut Navigator::default(), "Subject.java", text)
}

#[test]
fn self_call_to_transactional_method_warns_only_for_unique_name() {
    let host = MockHost::install();
    host.set_setting("springBeans.checkSelfInvocation", "true");
    assert_eq!(
        self_invocation(&host, SELF_CALL),
        [pair("self-invocation", "save"), pair("self-invocation", "save")]
    );
    let overloaded = SELF_CALL.replace("    public void run", "    void save(int x) {}\n    public void run");
    assert!(self_invocation(&host, &overloaded).is_empty());
    let inherited = SELF_CALL.replace("class A", "class A extends Base");
    assert!(self_invocation(&host, &inherited).is_empty());
}

#[test]
fn call_through_injected_self_is_quiet() {
    let host = MockHost::install();
    host.set_setting("springBeans.checkSelfInvocation", "true");
    let text = SELF_CALL.replace("        save();\n        this.save();\n", "        self.save();\n        other.save();\n");
    assert!(self_invocation(&host, &text).is_empty());
    let same_annotation = SELF_CALL.replace("    public void run", "    @Transactional\n    public void run");
    assert!(self_invocation(&host, &same_annotation).is_empty(), "a transactional caller already holds the transaction");
}

#[test]
fn self_invocation_is_off_by_default() {
    assert!(found(SELF_CALL).is_empty());
}

#[test]
fn self_invocation_is_quiet_for_library_or_dynamic_name() {
    let host = MockHost::install();
    host.set_setting("springBeans.checkSelfInvocation", "true");
    let library = SELF_CALL.replace("@Service", "import com.acme.Transactional;\n@Service");
    assert!(self_invocation(&host, &library).is_empty());
    let local = "@Service\nclass A {\n    @Transactional\n    public void save() {}\n    public void run(Runnable save) {\n        helper.save();\n        String s = \"save()\";\n    }\n}\n";
    assert!(self_invocation(&host, local).is_empty());
}

#[test]
fn switched_off_groups_are_quiet() {
    let host = MockHost::install();
    host.set_setting("springBeans.checkProxyMethods", "false");
    let text = "@Service\nclass A {\n    @Transactional\n    private void save() {}\n}\n";
    assert!(findings(&host, &mut Navigator::default(), "Subject.java", text).is_empty());
}

#[test]
fn fixed_file_publishes_empty_list() {
    let host = MockHost::install();
    let mut n = Navigator::default();
    let broken = "@Service\nclass A {\n    @Transactional\n    private void save() {}\n}\n";
    assert_eq!(findings(&host, &mut n, "Subject.java", broken).len(), 1);
    let fixed = broken.replace("private ", "");
    assert!(findings(&host, &mut n, "Subject.java", &fixed).is_empty());
    let path = java("Subject.java", "").path;
    assert!(host.diagnostics().get(&path).is_none_or(Vec::is_empty), "no stale problem is left");
}

#[test]
fn saved_and_opened_files_are_checked() {
    let host = MockHost::install();
    host.add_file("src/main/java/com/acme/Subject.java", "@Service\nclass A {\n    @Transactional\n    private void save() {}\n}\n");
    let mut n = Navigator::default();
    let path = java("Subject.java", "").path;
    n.handle_events(&[Event::FileOpened(path.clone())], &host.invocation());
    assert_eq!(host.diagnostics()[&path].len(), 1);
    host.add_file("src/main/java/com/acme/Subject.java", "@Service\nclass A {}\n");
    n.handle_events(&[Event::FileSaved(path.clone())], &host.invocation());
    assert!(host.diagnostics().get(&path).is_none_or(Vec::is_empty));
}

#[test]
fn ranges_use_utf16_columns_with_multibyte_text() {
    let host = MockHost::install();
    let text = "@Service\nclass A {\n    @Transactional\n    private void caf\u{e9}\u{1F600}() {}\n}\n";
    let document = java("Subject.java", text);
    Navigator::default().line_markers(&document);
    let d = &host.diagnostics()[&document.path][0];
    assert_eq!((d.range.start.line, d.range.start.character, d.range.end.character), (3, 17, 23));
}

fn unresolved(text: &str, extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let host = setup();
    host.set_setting("springBeans.checkUnresolvedReferences", "true");
    for (name, content) in extra {
        host.add_file(&format!("src/main/java/com/acme/{name}"), content);
    }
    findings(&host, &mut Navigator::default(), "Subject.java", text)
}

const BOOT_APP: (&str, &str) = ("App.java", "package com.acme;\n@SpringBootApplication\nclass App {}\n");

#[test]
fn depends_on_unknown_name_warns_when_enabled() {
    let text = "@Service\n@DependsOn({\"mail\", \"cache\"})\nclass A {}\n";
    assert_eq!(unresolved(text, &[]), [pair("unresolved-depends-on", "cache")]);
    assert!(found(text).is_empty(), "off by default");
}

#[test]
fn depends_on_is_quiet_when_project_uses_auto_configuration() {
    let library = "@Service\n@DependsOn(\"flyway\")\nclass A {}\n";
    assert!(unresolved(library, &[BOOT_APP]).is_empty());
    let enable = ("Cfg.java", "package com.acme;\n@EnableAutoConfiguration\nclass Cfg {}\n");
    assert!(unresolved(library, &[enable]).is_empty());
    let project_shaped = "@Service\n@DependsOn(\"abstractNotifier\")\nclass A {}\n";
    assert_eq!(unresolved(project_shaped, &[BOOT_APP]), [pair("unresolved-depends-on", "abstractNotifier")]);
}

#[test]
fn depends_on_known_xml_bean_is_quiet() {
    assert!(unresolved("@Service\n@DependsOn({\"legacy\", \"fake\", \"mailer\", \"users\"})\nclass A {}\n", &[]).is_empty());
}

#[test]
fn depends_on_is_quiet_for_library_or_dynamic_name() {
    assert!(unresolved("@Service\n@DependsOn(Names.CACHE)\nclass A {}\n", &[]).is_empty());
    assert!(unresolved("import com.acme.DependsOn;\n@Service\n@DependsOn(\"cache\")\nclass A {}\n", &[]).is_empty());
}

#[test]
fn component_scan_base_package_without_matching_sources_warns() {
    let text = "@Configuration\n@ComponentScan(basePackages = {\"com.acme\", \"com.acme.missing\"})\nclass A {}\n";
    assert_eq!(unresolved(text, &[]), [pair("component-scan-package-missing", "com.acme.missing")]);
    assert_eq!(unresolved("@ComponentScan(\"com.acme.gone\")\nclass A {}\n", &[]).len(), 1);
}

#[test]
fn component_scan_with_class_reference_is_quiet() {
    let text = "@ComponentScan(basePackageClasses = App.class, basePackages = \"com.acme.missing\")\nclass A {}\n";
    assert!(unresolved(text, &[]).is_empty());
}

#[test]
fn component_scan_is_quiet_for_library_or_dynamic_name() {
    assert!(unresolved("@ComponentScan(\"org.vendor.lib\")\nclass A {}\n", &[]).is_empty(), "a package outside the project's root");
    assert!(unresolved("@ComponentScan(Packages.ROOT)\nclass A {}\n", &[]).is_empty());
    assert!(unresolved("import com.acme.ComponentScan;\n@ComponentScan(\"com.acme.missing\")\nclass A {}\n", &[]).is_empty());
}

#[test]
fn unresolved_references_fixed_file_publishes_empty_list() {
    let host = setup();
    host.set_setting("springBeans.checkUnresolvedReferences", "true");
    let mut n = Navigator::default();
    let broken = "@Service\n@DependsOn(\"cache\")\nclass A {}\n";
    assert_eq!(findings(&host, &mut n, "Subject.java", broken).len(), 1);
    assert!(findings(&host, &mut n, "Subject.java", &broken.replace("cache", "mail")).is_empty());
}

#[test]
fn unresolved_reference_ranges_use_utf16_columns() {
    let host = setup();
    host.set_setting("springBeans.checkUnresolvedReferences", "true");
    let text = "@Service\n@DependsOn(\"caf\u{e9}\u{1F600}\")\nclass A {}\n";
    let document = java("Subject.java", text);
    Navigator::default().line_markers(&document);
    let d = &host.diagnostics()[&document.path][0];
    assert_eq!((d.range.start.character, d.range.end.character), (12, 18));
}

fn findings_in(host: &MockHost, n: &mut Navigator, relative: &str, text: &str) -> Vec<(String, String)> {
    host.add_file(relative, text);
    let document = doc(&format!("/project/{relative}"), text);
    n.line_markers(&document);
    host.diagnostics()
        .get(&document.path)
        .into_iter()
        .flatten()
        .filter(|d| d.code.as_deref() == Some("duplicate-bean-name"))
        .map(|d| (d.code.clone().unwrap(), d.message.clone()))
        .collect()
}

#[test]
fn nested_configuration_classes_do_not_clash() {
    let host = setup();
    let mut n = Navigator::default();
    let nested = |outer: &str| format!("class {outer} {{\n    @Configuration\n    static class Config {{}}\n}}\n");
    let a = nested("ATests");
    let b = nested("BTests");
    assert!(findings_in(&host, &mut n, "src/test/java/com/acme/ATests.java", &a).is_empty());
    assert!(findings_in(&host, &mut n, "src/test/java/com/acme/BTests.java", &b).is_empty());
    assert!(findings_in(&host, &mut n, "src/test/java/com/acme/ATests.java", &a).is_empty());
}

#[test]
fn same_simple_name_in_different_modules_is_quiet() {
    let host = setup();
    host.add_file("one/pom.xml", "<project/>");
    host.add_file("two/pom.xml", "<project/>");
    let app = "@SpringBootApplication\nclass Application {}\n";
    let mut n = Navigator::default();
    findings_in(&host, &mut n, "two/src/main/java/org/two/Application.java", app);
    assert!(findings_in(&host, &mut n, "one/src/main/java/org/one/Application.java", app).is_empty());
    assert!(findings_in(&host, &mut n, "two/src/main/java/org/two/Application.java", app).is_empty());
}

#[test]
fn same_name_in_main_and_test_source_sets_is_quiet() {
    let host = setup();
    let service = "@Service(\"dispatcher\")\nclass Dispatcher {}\n";
    let mut n = Navigator::default();
    findings_in(&host, &mut n, "src/test/java/com/acme/Dispatcher.java", service);
    assert!(findings_in(&host, &mut n, "src/main/java/com/acme/Dispatcher.java", service).is_empty());
}

#[test]
fn same_name_in_same_module_and_source_set_still_warns() {
    let host = setup();
    host.add_file("one/pom.xml", "<project/>");
    let service = "@Service(\"dispatcher\")\nclass Dispatcher {}\n";
    let other = "@Service(\"dispatcher\")\nclass Other {}\n";
    let mut n = Navigator::default();
    findings_in(&host, &mut n, "one/src/main/java/org/one/other/Other.java", other);
    assert_eq!(findings_in(&host, &mut n, "one/src/main/java/org/one/Dispatcher.java", service).len(), 1);
}

#[test]
fn unresolved_references_are_quiet_on_a_partial_index() {
    let host = setup();
    host.set_setting("springBeans.checkUnresolvedReferences", "true");
    let text = "@Service\n@DependsOn(\"cache\")\nclass A {}\n";
    let mut index = spring_text::index::Index::new(GLOBS, model::parse);
    index.update(&format!("{ROOT}/Subject.java"), text);
    assert!(!index.complete());
    let file = java::parse(text);
    let world = World::new(&index);
    let found = checks::check_java(&format!("{ROOT}/Subject.java"), &file, text, &world, checks::Groups::from_settings());
    assert!(found.iter().all(|f| f.code != "unresolved-depends-on"));
}

const XML_PATH: &str = "/project/src/main/resources/aop.xml";

fn beans_xml(body: &str) -> String {
    format!(
        "<beans xmlns=\"http://www.springframework.org/schema/beans\"\n       xmlns:aop=\"http://www.springframework.org/schema/aop\">\n{body}\n</beans>\n"
    )
}

fn xml_findings(host: &MockHost, text: &str) -> Vec<(String, String)> {
    let document = doc(XML_PATH, text);
    Navigator::default().line_markers(&document);
    host.diagnostics()
        .get(XML_PATH)
        .into_iter()
        .flatten()
        .map(|d| {
            let (from, to) = (offset(text, d.range.start).unwrap(), offset(text, d.range.end).unwrap());
            (d.code.clone().unwrap(), text[from..to].to_owned())
        })
        .collect()
}

fn aop_found(body: &str) -> Vec<(String, String)> {
    xml_findings(&MockHost::install(), &beans_xml(body))
}

#[test]
fn aop_pointcut_without_expression_is_reported() {
    let found = aop_found("<aop:config><aop:pointcut id=\"p\"/></aop:config>");
    assert_eq!(found, [pair("aop-structure", "aop:pointcut")]);
    assert_eq!(aop_found("<aop:config><aop:pointcut expression=\"within(a..*)\"/></aop:config>").len(), 1);
}

#[test]
fn aop_aspect_without_ref_is_reported() {
    let found = aop_found("<aop:config><aop:aspect id=\"a\"/></aop:config>");
    assert_eq!(found, [pair("aop-structure", "aop:aspect")]);
}

#[test]
fn aop_advice_without_pointcut_is_reported() {
    let found = aop_found("<aop:config><aop:aspect ref=\"a\"><aop:before method=\"m\"/></aop:aspect></aop:config>");
    assert_eq!(found, [pair("aop-structure", "aop:before")]);
    let no_method = aop_found("<aop:config><aop:aspect ref=\"a\"><aop:around pointcut-ref=\"p\"/></aop:aspect></aop:config>");
    assert_eq!(no_method, [pair("aop-structure", "aop:around")]);
}

#[test]
fn aop_advice_with_both_pointcut_and_pointcut_ref_is_reported() {
    let found = aop_found("<aop:config><aop:aspect ref=\"a\"><aop:after method=\"m\" pointcut=\"x\" pointcut-ref=\"p\"/></aop:aspect></aop:config>");
    assert_eq!(found, [pair("aop-structure", "aop:after")]);
}

#[test]
fn aop_advisor_without_advice_ref_is_reported() {
    let found = aop_found("<aop:config><aop:advisor pointcut-ref=\"p\"/></aop:config>");
    assert_eq!(found, [pair("aop-structure", "aop:advisor")]);
}

#[test]
fn aop_structure_is_quiet_for_library_or_dynamic_name() {
    let dynamic = "<aop:config><aop:aspect ref=\"${aspect}\"><aop:before method=\"m\" pointcut=\"${a}\" pointcut-ref=\"${b}\"/></aop:aspect></aop:config>";
    assert!(aop_found(dynamic).is_empty());
    let lookalike = "<x:config xmlns:x=\"urn:other\"><x:pointcut/><x:aspect/><x:advisor/></x:config>";
    assert!(aop_found(lookalike).is_empty());
    let outside_config = "<aop:pointcut id=\"p\"/>";
    assert!(aop_found(outside_config).is_empty());
    let dynamic_pair = "<aop:config><aop:aspect ref=\"a\"><aop:before method=\"m\" pointcut=\"x\" pointcut-ref=\"#{b}\"/></aop:aspect></aop:config>";
    assert!(aop_found(dynamic_pair).is_empty());
    let not_beans = "<project xmlns:aop=\"http://www.springframework.org/schema/aop\"><aop:config><aop:aspect/></aop:config></project>";
    assert!(xml_findings(&MockHost::install(), not_beans).is_empty());
}

#[test]
fn bean_with_class_and_factory_bean_is_reported() {
    let found = aop_found("<bean id=\"a\" class=\"x.Y\" factory-bean=\"f\" factory-method=\"make\"/>");
    assert_eq!(found, [pair("xml-bean-attributes", "factory-bean")]);
}

#[test]
fn factory_bean_without_factory_method_is_reported() {
    let found = aop_found("<bean id=\"a\" factory-bean=\"f\"/>");
    assert_eq!(found, [pair("xml-bean-attributes", "factory-bean")]);
}

#[test]
fn bean_without_class_parent_or_factory_bean_is_reported() {
    let found = aop_found("<bean id=\"a\"/>");
    assert_eq!(found, [pair("xml-bean-attributes", "bean")]);
    assert!(aop_found("<bean id=\"a\" parent=\"b\"/>").is_empty());
}

#[test]
fn abstract_bean_without_class_is_quiet() {
    assert!(aop_found("<bean id=\"a\" abstract=\"true\"/>").is_empty());
}

#[test]
fn property_with_ref_and_value_is_reported() {
    let found = aop_found("<bean class=\"x.Y\"><property name=\"p\" ref=\"a\" value=\"b\"/></bean>");
    assert_eq!(found, [pair("xml-injection-value", "property")]);
    let arg = aop_found("<bean class=\"x.Y\"><constructor-arg ref=\"a\" value=\"b\"/></bean>");
    assert_eq!(arg, [pair("xml-injection-value", "constructor-arg")]);
}

#[test]
fn property_with_attribute_and_child_element_is_reported() {
    let found = aop_found("<bean class=\"x.Y\"><property name=\"p\" value=\"b\"><list/></property></bean>");
    assert_eq!(found, [pair("xml-injection-value", "property")]);
    let described = aop_found("<bean class=\"x.Y\"><property name=\"p\" value=\"b\"><description>d</description></property></bean>");
    assert!(described.is_empty());
}

#[test]
fn property_with_nothing_is_reported() {
    let found = aop_found("<bean class=\"x.Y\"><property name=\"p\"/></bean>");
    assert_eq!(found, [pair("xml-injection-value", "property")]);
    assert!(aop_found("<bean class=\"x.Y\"><property name=\"p\"><null/></property></bean>").is_empty());
}

#[test]
fn constructor_arg_without_value_is_quiet() {
    assert!(aop_found("<bean class=\"x.Y\"><constructor-arg index=\"0\"/></bean>").is_empty());
}

#[test]
fn misspelled_scope_is_reported() {
    let found = aop_found("<bean class=\"x.Y\" scope=\"Singleton\"/>");
    assert_eq!(found, [pair("xml-scope", "Singleton")]);
    let typo = aop_found("<bean class=\"x.Y\" scope=\"protoype\"/>");
    assert_eq!(typo, [pair("xml-scope", "protoype")]);
}

#[test]
fn bundle_scope_is_accepted() {
    assert!(aop_found("<bean class=\"x.Y\" scope=\"bundle\"/>").is_empty());
}

#[test]
fn custom_scope_is_quiet() {
    assert!(aop_found("<bean class=\"x.Y\" scope=\"thread\"/>").is_empty());
    assert!(aop_found("<bean class=\"x.Y\" scope=\"custom\"/>").is_empty());
}

#[test]
fn xml_attribute_checks_are_quiet_for_library_or_dynamic_name() {
    let dynamic = "<bean id=\"a\" class=\"${c}\" factory-bean=\"#{f}\" scope=\"${Singleton}\"><property name=\"p\" ref=\"${r}\" value=\"v\"/></bean>";
    assert!(aop_found(dynamic).is_empty());
    let other_ns = "<x:bean xmlns:x=\"urn:other\" id=\"a\" scope=\"Singleton\"><x:property name=\"p\"/></x:bean>";
    assert!(aop_found(other_ns).is_empty());
    let other_file = "<project xmlns=\"urn:other\"><bean id=\"a\" scope=\"Singleton\"><property name=\"p\"/></bean></project>";
    assert!(xml_findings(&MockHost::install(), other_file).is_empty());
}

#[test]
fn xml_attribute_checks_use_utf16_columns_and_crlf_lines() {
    let host = MockHost::install();
    let text = beans_xml("<!-- caf\u{e9}\u{1F600} --><bean id=\"a\"/>");
    xml_findings(&host, &text);
    let d = &host.diagnostics()[XML_PATH][0];
    assert_eq!((d.range.start.line, d.range.start.character, d.range.end.character), (2, 16, 20));
    let text = beans_xml("<bean class=\"x.Y\"\n scope=\"Singleton\"/>").replace('\n', "\r\n");
    xml_findings(&host, &text);
    let d = &host.diagnostics()[XML_PATH][0];
    assert_eq!((d.range.start.line, d.range.start.character, d.range.end.character), (3, 8, 17));
}

#[test]
fn xml_bean_name_with_capital_is_hinted() {
    let found = aop_found("<bean id=\"Dispatcher\" name=\"alias, Other\" class=\"x.Y\"/>");
    assert_eq!(found, [pair("bean-name-convention", "Dispatcher"), pair("bean-name-convention", "Other")]);
}

#[test]
fn duplicate_xml_ids_in_one_file_are_reported() {
    let found = aop_found("<bean id=\"a\" class=\"x.Y\"/>\n<bean id=\"b\" name=\"a\" class=\"x.Y\"/>");
    assert_eq!(found, [pair("duplicate-bean-name", "a")]);
    let host = MockHost::install();
    let text = beans_xml("<bean id=\"a\" class=\"x.Y\"/>\n<bean id=\"a\" class=\"x.Y\"/>");
    xml_findings(&host, &text);
    let d = &host.diagnostics()[XML_PATH][0];
    assert_eq!((d.range.start.line, d.range.start.character), (3, 10));
}

#[test]
fn xml_bean_clashing_with_java_bean_is_reported() {
    let host = MockHost::install();
    host.add_file("src/main/java/com/acme/Dispatcher.java", "@Service(\"dispatcher\")\nclass Dispatcher {}\n");
    let found = xml_findings(&host, &beans_xml("<bean id=\"dispatcher\" class=\"x.Y\"/>"));
    assert_eq!(found, [pair("duplicate-bean-name", "dispatcher")]);
    let host = MockHost::install();
    host.add_file("src/main/java/com/acme/Dispatcher.java", "@Service(\"dispatcher\")\n@Profile(\"dev\")\nclass Dispatcher {}\n");
    let other_profile = beans_xml("<beans profile=\"prod\"><bean id=\"dispatcher\" class=\"x.Y\"/></beans>");
    assert!(xml_findings(&host, &other_profile).is_empty());
}

#[test]
fn same_id_in_different_profile_blocks_is_quiet() {
    let body = "<beans profile=\"dev\"><bean id=\"a\" class=\"x.Y\"/></beans><beans profile=\"prod\"><bean id=\"a\" class=\"x.Z\"/></beans>";
    assert!(aop_found(body).is_empty());
}

#[test]
fn same_id_in_two_xml_files_is_quiet() {
    let host = MockHost::install();
    host.add_file("src/main/resources/other.xml", &beans_xml("<bean id=\"a\" class=\"x.Y\"/>"));
    assert!(xml_findings(&host, &beans_xml("<bean id=\"a\" class=\"x.Z\"/>")).is_empty());
}

#[test]
fn generated_names_are_quiet() {
    let body = "<bean id=\"x.Y#0\" class=\"x.Y\"/><bean id=\"x.Y#0\" class=\"x.Y\"/>";
    assert!(aop_found(body).is_empty());
}

#[test]
fn xml_names_are_quiet_for_library_or_dynamic_name() {
    let dynamic = "<bean id=\"${A}\" class=\"x.Y\"/><bean id=\"${A}\" class=\"x.Y\"/><bean id=\"A#{b}\" class=\"x.Y\"/>";
    assert!(aop_found(dynamic).is_empty());
    let other_file = "<project xmlns=\"urn:other\"><bean id=\"A\"/><bean id=\"A\"/></project>";
    assert!(xml_findings(&MockHost::install(), other_file).is_empty());
    let host = MockHost::install();
    host.set_setting("springBeans.checkNaming", "false");
    host.set_setting("springBeans.checkDuplicateNames", "false");
    assert!(xml_findings(&host, &beans_xml("<bean id=\"A\" class=\"x.Y\"/><bean id=\"A\" class=\"x.Y\"/>")).is_empty());
}

#[test]
fn ref_to_abstract_bean_is_reported() {
    let body = "<bean id=\"base\" abstract=\"true\"/>\n<bean id=\"a\" class=\"x.Y\" depends-on=\"base\"><property name=\"p\" ref=\"base\"/><property name=\"q\"><ref bean=\"base\"/></property></bean>";
    let found: Vec<_> = aop_found(body).into_iter().filter(|(code, _)| code == "xml-abstract-reference").collect();
    assert_eq!(found, vec![pair("xml-abstract-reference", "base"); 3]);
}

#[test]
fn parent_to_abstract_bean_is_quiet() {
    assert!(aop_found("<bean id=\"base\" abstract=\"true\"/><bean id=\"a\" parent=\"base\"/>").is_empty());
}

#[test]
fn ref_to_bean_with_a_concrete_definition_is_quiet() {
    let host = MockHost::install();
    host.add_file("src/main/java/com/acme/Base.java", "@Service(\"base\")\nclass Base {}\n");
    let body = "<bean id=\"base\" abstract=\"true\"/><bean id=\"a\" class=\"x.Y\"><property name=\"p\" ref=\"base\"/></bean>";
    let found = xml_findings(&host, &beans_xml(body));
    assert!(found.iter().all(|(code, _)| code != "xml-abstract-reference"));
}

#[test]
fn xml_name_findings_use_utf16_columns_and_crlf_lines() {
    let host = MockHost::install();
    let text = beans_xml("<!-- caf\u{e9}\u{1F600} --><bean id=\"Ab\" class=\"x.Y\"/>");
    xml_findings(&host, &text);
    let d = &host.diagnostics()[XML_PATH][0];
    assert_eq!((d.range.start.line, d.range.start.character, d.range.end.character), (2, 25, 27));
    let text = beans_xml("<bean id=\"base\" abstract=\"true\"/>\n<bean class=\"x.Y\"\n  depends-on=\"base\"/>").replace('\n', "\r\n");
    xml_findings(&host, &text);
    let d = &host.diagnostics()[XML_PATH][0];
    assert_eq!((d.range.start.line, d.range.start.character, d.range.end.character), (4, 14, 18));
}

#[test]
fn switched_off_xml_group_is_quiet() {
    let host = MockHost::install();
    host.set_setting("springBeans.checkXml", "false");
    assert!(xml_findings(&host, &beans_xml("<bean id=\"a\"/>")).is_empty());
}

#[test]
fn xml_ranges_use_utf16_columns_with_multibyte_text() {
    let host = MockHost::install();
    let text = beans_xml("<!-- caf\u{e9}\u{1F600} --><aop:config><aop:aspect id=\"a\"/></aop:config>");
    xml_findings(&host, &text);
    let d = &host.diagnostics()[XML_PATH][0];
    assert_eq!((d.range.start.line, d.range.start.character, d.range.end.character), (2, 28, 38));
}

#[test]
fn xml_crlf_files_report_correct_lines() {
    let host = MockHost::install();
    let text = beans_xml("<aop:config>\n<aop:aspect id=\"a\"/>\n</aop:config>").replace('\n', "\r\n");
    xml_findings(&host, &text);
    let d = &host.diagnostics()[XML_PATH][0];
    assert_eq!((d.range.start.line, d.range.start.character), (3, 1));
}

#[test]
fn fixed_xml_file_publishes_empty_list() {
    let host = MockHost::install();
    let broken = beans_xml("<aop:config><aop:aspect id=\"a\"/></aop:config>");
    assert_eq!(xml_findings(&host, &broken).len(), 1);
    let fixed = broken.replace("id=\"a\"", "ref=\"a\"");
    assert!(xml_findings(&host, &fixed).is_empty());
    assert!(host.diagnostics().get(XML_PATH).is_none_or(Vec::is_empty), "no stale problem is left");
}

#[test]
fn switched_off_aop_group_is_quiet() {
    let host = MockHost::install();
    host.set_setting("springBeans.checkAop", "false");
    assert!(xml_findings(&host, &beans_xml("<aop:config><aop:aspect id=\"a\"/></aop:config>")).is_empty());
}

#[test]
fn saved_xml_is_checked_and_pom_is_not() {
    let host = MockHost::install();
    let broken = beans_xml("<aop:config><aop:aspect id=\"a\"/></aop:config>");
    host.add_file("src/main/resources/aop.xml", &broken);
    host.add_file("pom.xml", "<project><bean id=\"a\" class=\"x.Y\"/></project>");
    let mut n = Navigator::default();
    n.handle_events(&[Event::FileSaved(XML_PATH.into())], &host.invocation());
    assert_eq!(host.diagnostics()[XML_PATH].len(), 1);
    n.handle_events(&[Event::FileSaved("/project/pom.xml".into())], &host.invocation());
    assert!(host.diagnostics().get("/project/pom.xml").is_none_or(Vec::is_empty));
}

const WEAVING_ASPECT: &str = "import org.aspectj.lang.annotation.*;\n\n@Aspect\nclass Tracing {\n    @Before(\"within(a..*) && call(* *(..))\")\n    void trace() {}\n}\n";

#[test]
fn unsupported_designator_in_java_advice_is_reported() {
    assert_eq!(found(WEAVING_ASPECT), [pair("unsupported-pointcut-designator", "call")]);
    let utf16 = "@Aspect\nclass T {\n    @Pointcut(\"within(é..*) && cflow(x())\")\n    void p() {}\n}\n";
    let host = MockHost::install();
    let document = java("Subject.java", utf16);
    Navigator::default().line_markers(&document);
    let diagnostic = &host.diagnostics()[&document.path][0];
    assert_eq!((diagnostic.range.start.line, diagnostic.range.start.character), (2, 31));
    assert_eq!(diagnostic.range.end.character, 36);
}

#[test]
fn unsupported_designator_in_xml_expression_is_reported() {
    let body = "<aop:config><aop:pointcut id=\"p\" expression=\"call(* *(..))\"/>\
        <aop:aspect ref=\"a\"><aop:before method=\"m\" pointcut=\"within(a..*) &amp;&amp; get(* x)\"/></aop:aspect></aop:config>";
    assert_eq!(aop_found(body), [pair("unsupported-pointcut-designator", "call"), pair("unsupported-pointcut-designator", "get")]);
    assert!(aop_found("<aop:config><aop:pointcut id=\"p\" expression=\"execution(* get(..))\"/></aop:config>").is_empty());
}

#[test]
fn unsupported_designator_is_quiet_for_library_or_dynamic_name() {
    let concatenated = WEAVING_ASPECT.replace("\"within(a..*) && call(* *(..))\"", "\"within(a..*) && \" + \"call(* *(..))\"");
    assert!(found(&concatenated).is_empty());
    let placeholder = WEAVING_ASPECT.replace("call(* *(..))", "${expr}");
    assert!(found(&placeholder).is_empty());
    let foreign = "import org.junit.Before;\n\n@Aspect\nclass T {\n    @Before(\"call(* *(..))\")\n    void p() {}\n}\n";
    assert!(found(foreign).is_empty());
    let not_an_aspect = "class T {\n    @Before(\"call(* *(..))\")\n    void p() {}\n}\n";
    assert!(found(not_an_aspect).is_empty());
    let host = MockHost::install();
    host.add_file("pom.xml", "<project><build><plugin>aspectj-maven-plugin</plugin></build></project>");
    assert!(findings(&host, &mut Navigator::default(), "Subject.java", WEAVING_ASPECT).is_empty());
    let xml_host = MockHost::install();
    xml_host.add_file("pom.xml", "<project>aspectjtools</project>");
    let body = "<aop:config><aop:pointcut id=\"p\" expression=\"call(* *(..))\"/></aop:config>";
    assert!(xml_findings(&xml_host, &beans_xml(body)).is_empty());
    assert!(aop_found("<aop:config><aop:pointcut id=\"p\" expression=\"${expr}\"/></aop:config>").is_empty());
}

#[test]
fn crlf_files_report_correct_lines() {
    let text = "@Aspect\r\nclass T {\r\n    @Pointcut(\"within(a..*) && cflow(x())\")\r\n    void p() {}\r\n}\r\n";
    let document = java("Subject.java", text);
    let host = MockHost::install();
    Navigator::default().line_markers(&document);
    let range = host.diagnostics()[&document.path][0].range;
    assert_eq!((range.start.line, range.start.character, range.end.line, range.end.character), (2, 31, 2, 36));

    let xml = beans_xml("<aop:config>\r\n<aop:pointcut id=\"p\" expression=\"within(a..*) &amp;&amp; call(* *(..))\"/>\r\n</aop:config>").replace('\n', "\r\n");
    let host = MockHost::install();
    Navigator::default().line_markers(&doc(XML_PATH, &xml));
    let range = host.diagnostics()[XML_PATH][0].range;
    assert_eq!((range.start.line, range.start.character, range.end.line, range.end.character), (3, 57, 3, 61));
}

#[test]
fn escaped_literal_is_quiet() {
    let escaped = WEAVING_ASPECT.replace("within(a..*) &&", "within(a..*) \\t&&");
    assert!(found(&escaped).is_empty());
}

fn applied_xml_fix(text: &str) -> Option<String> {
    let host = MockHost::install();
    let document = doc(XML_PATH, text);
    let mut navigator = Navigator::default();
    navigator.line_markers(&document);
    let hint = host.diagnostics()[XML_PATH].iter().find(|d| d.code.as_deref() == Some("xml-value-style"))?.range;
    let actions = navigator.code_actions(&document, hint);
    let edits = &actions.first()?.edits;
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].0, XML_PATH);
    let mut fixed = text.to_owned();
    for e in &edits[0].1 {
        let (s, t) = (offset(&fixed, e.range.start).unwrap(), offset(&fixed, e.range.end).unwrap());
        fixed.replace_range(s..t, &e.text);
    }
    Some(fixed)
}

#[test]
fn value_element_is_hinted_and_rewritten_to_attribute() {
    let text = beans_xml("<bean id=\"a\" class=\"x.Y\">\n<property name=\"n\">\n  <value>5 &amp; 6</value>\n</property>\n</bean>");
    let host = MockHost::install();
    assert_eq!(xml_findings(&host, &text), [pair("xml-value-style", "value")]);
    assert_eq!(host.diagnostics()[XML_PATH][0].severity, Severity::Hint);
    let constructor = beans_xml("<bean id=\"a\" class=\"x.Y\"><constructor-arg index=\"0\"><value>7</value></constructor-arg></bean>");
    assert_eq!(xml_findings(&host, &constructor), [pair("xml-value-style", "value")]);
    let fixed = applied_xml_fix(&text).unwrap();
    assert!(fixed.contains("<property name=\"n\" value=\"5 &amp; 6\"/>\n</bean>"), "{fixed}");
    let els = spring_text::xml::elements(&fixed);
    let property = els.iter().find(|e| e.name == "property").unwrap();
    assert_eq!(property.value("value"), Some("5 &amp; 6"));
    assert!(els.iter().all(|e| e.name != "value"));
}

#[test]
fn ref_element_is_hinted_and_rewritten_to_attribute() {
    let text = beans_xml("<bean id=\"a\" class=\"x.Y\">\n<property name=\"n\"><ref bean=\"other\"/></property>\n</bean>");
    let host = MockHost::install();
    assert_eq!(xml_findings(&host, &text), [pair("xml-value-style", "ref")]);
    let fixed = applied_xml_fix(&text).unwrap();
    assert!(fixed.contains("<property name=\"n\" ref=\"other\"/>\n</bean>"), "{fixed}");
    let els = spring_text::xml::elements(&fixed);
    let property = els.iter().find(|e| e.name == "property").unwrap();
    assert_eq!(property.value("ref"), Some("other"));
    assert!(els.iter().all(|e| e.name != "ref"));
    assert!(els.iter().any(|e| e.name == "bean"));
}

#[test]
fn value_style_is_quiet_for_library_or_dynamic_name() {
    let host = MockHost::install();
    let quiet = |body: &str| {
        let found = xml_findings(&host, &beans_xml(&format!("<bean id=\"a\" class=\"x.Y\">{body}</bean>")));
        assert!(found.iter().all(|(code, _)| code != "xml-value-style"), "{body}: {found:?}");
    };
    quiet("<property name=\"n\"><value>one\ntwo</value></property>");
    quiet("<property name=\"n\"><value><![CDATA[a<b]]></value></property>");
    quiet("<property name=\"n\"><value type=\"java.lang.String\">v</value></property>");
    quiet("<property name=\"n\"><value>say \"hi\"</value></property>");
    quiet("<property name=\"n\"><value>a</value><value>b</value></property>");
    quiet("<property name=\"n\"><value>a</value><description>d</description></property>");
    quiet("<property name=\"n\"><ref local=\"o\"/></property>");
    quiet("<property name=\"n\"><ref bean=\"o\" local=\"o\"/></property>");
    quiet("<property name=\"n\"><ref bean=\"${o}\"/></property>");
    quiet("<property name=\"n\"><value>${v}</value></property>");
    quiet("<property name=\"n\" value=\"x\"><value>a</value></property>");
    quiet("<property name=\"n\"/>");
    quiet("<property name=\"n\"><!-- keep --><value>a</value></property>");
    let other_ns = "<x:bean xmlns:x=\"urn:other\"><x:property name=\"n\"><x:value>a</x:value></x:property></x:bean>";
    assert!(xml_findings(&host, other_ns).is_empty());
    host.set_setting("springBeans.checkXml", "false");
    let off = beans_xml("<bean id=\"a\" class=\"x.Y\"><property name=\"n\"><value>a</value></property></bean>");
    assert!(xml_findings(&host, &off).is_empty());
}

#[test]
fn rewrite_keeps_multibyte_text() {
    let text = beans_xml("<!-- \u{1F600} --><bean id=\"a\" class=\"x.Y\">\n<property name=\"n\"><value>caf\u{e9} \u{1F600}</value></property>\n</bean>").replace('\n', "\r\n");
    let host = MockHost::install();
    xml_findings(&host, &text);
    let hint = host.diagnostics()[XML_PATH][0].range;
    assert_eq!((hint.start.line, hint.start.character), (3, 20));
    let fixed = applied_xml_fix(&text).unwrap();
    assert!(fixed.contains("<property name=\"n\" value=\"caf\u{e9} \u{1F600}\"/>\r\n</bean>"), "{fixed:?}");
    let els = spring_text::xml::elements(&fixed);
    assert_eq!(els.iter().find(|e| e.name == "property").unwrap().value("value"), Some("caf\u{e9} \u{1F600}"));
}

#[test]
fn code_action_applies_the_edit_to_the_open_file() {
    let text = beans_xml("<bean id=\"a\" class=\"x.Y\"><property name=\"n\"><value>v</value></property></bean>");
    let host = MockHost::install();
    let document = doc(XML_PATH, &text);
    let mut navigator = Navigator::default();
    navigator.line_markers(&document);
    let hint = host.diagnostics()[XML_PATH][0].range;
    let actions = navigator.code_actions(&document, hint);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].title, "Use the value attribute");
    assert_eq!(actions[0].edits.len(), 1);
    assert_eq!(actions[0].edits[0].0, XML_PATH);
    let elsewhere = Range::new(Position::new(0, 1), Position::new(0, 1));
    assert!(navigator.code_actions(&document, elsewhere).is_empty());

    let reference = beans_xml("<bean id=\"a\" class=\"x.Y\"><property name=\"n\"><ref bean=\"o\"/></property></bean>");
    let document = doc(XML_PATH, &reference);
    navigator.line_markers(&document);
    let hint = host.diagnostics()[XML_PATH][0].range;
    assert_eq!(navigator.code_actions(&document, hint)[0].title, "Use the ref attribute");
}

fn apply_edits(text: &str, edits: &[TextEdit]) -> String {
    let mut fixed = text.to_owned();
    for e in edits {
        let (s, t) = (offset(&fixed, e.range.start).unwrap(), offset(&fixed, e.range.end).unwrap());
        fixed.replace_range(s..t, &e.text);
    }
    fixed
}

fn caret_in(text: &str, needle: &str) -> usize {
    text.find(needle).unwrap() + 2
}

const P_NS: &str = "http://www.springframework.org/schema/p";

#[test]
fn p_namespace_moves_value_and_adds_declaration() {
    let text = beans_xml("<bean id=\"a\" class=\"x.Y\">\n  <property name=\"n\" value=\"5\"/>\n  <property name=\"m\" value=\"6\"/>\n</bean>");
    let edits = xml_actions::use_p_namespace(&text, caret_in(&text, "<property name=\"n\"")).unwrap();
    let fixed = apply_edits(&text, &edits);
    assert!(fixed.contains("class=\"x.Y\" p:n=\"5\">\n  <property name=\"m\" value=\"6\"/>\n</bean>"), "{fixed}");
    let els = xml::elements(&fixed);
    assert_eq!(els[0].value("xmlns:p"), Some(P_NS));
    assert_eq!(els.iter().find(|e| e.name == "bean").unwrap().value("p:n"), Some("5"));
    assert_eq!(els.iter().filter(|e| e.name == "property").count(), 1);
    assert_eq!(xml::elements(&fixed).len(), 3);
}

#[test]
fn p_namespace_reuses_bound_prefix() {
    let text = format!("<beans xmlns=\"http://www.springframework.org/schema/beans\" xmlns:q='{P_NS}'>\n<bean id='a' class='x.Y'><property name='n' value='say \"hi\"'/></bean>\n</beans>");
    let edits = xml_actions::use_p_namespace(&text, caret_in(&text, "<property")).unwrap();
    let fixed = apply_edits(&text, &edits);
    let els = xml::elements(&fixed);
    assert_eq!(els.iter().find(|e| e.name == "bean").unwrap().value("q:n"), Some("say \"hi\""));
    assert!(els[0].attrs.iter().all(|a| a.name != "xmlns:p"));
    assert!(els.iter().all(|e| e.name != "property"));
}

#[test]
fn p_namespace_for_ref_uses_ref_suffix() {
    let text = beans_xml("<bean id=\"a\" class=\"x.Y\"><property name=\"n\" ref=\"b\"/></bean>");
    let edits = xml_actions::use_p_namespace(&text, caret_in(&text, "<property")).unwrap();
    let els = xml::elements(&apply_edits(&text, &edits));
    assert_eq!(els.iter().find(|e| e.name == "bean").unwrap().value("p:n-ref"), Some("b"));
}

#[test]
fn p_namespace_is_quiet_for_library_or_dynamic_name() {
    let quiet = |text: &str| assert!(xml_actions::use_p_namespace(text, caret_in(text, "<property")).is_none(), "{text}");
    quiet(&beans_xml("<bean id=\"a\" class=\"x.Y\"><property name=\"my.n\" value=\"5\"/></bean>"));
    quiet(&beans_xml("<bean id=\"a\" class=\"x.Y\"><property name=\"my-n\" value=\"5\"/></bean>"));
    quiet(&beans_xml("<bean id=\"a\" class=\"x.Y\"><property name=\"${n}\" value=\"5\"/></bean>"));
    quiet(&beans_xml("<bean id=\"a\" class=\"x.Y\"><property name=\"n\" value=\"5\" ref=\"b\"/></bean>"));
    quiet(&beans_xml("<bean id=\"a\" class=\"x.Y\"><property name=\"n\"><value>5</value></property></bean>"));
    quiet(&format!("<beans xmlns=\"http://www.springframework.org/schema/beans\" xmlns:p=\"{P_NS}\"><bean id=\"a\" class=\"x.Y\" p:n=\"1\"><property name=\"n\" value=\"5\"/></bean></beans>"));
    quiet("<beans xmlns=\"http://www.springframework.org/schema/beans\" xmlns:p=\"urn:other\"><bean id=\"a\" class=\"x.Y\"><property name=\"n\" value=\"5\"/></bean></beans>");
    quiet("<beans xmlns=\"http://www.springframework.org/schema/beans\"><x:bean xmlns:x=\"urn:o\"><property name=\"n\" value=\"5\"/></x:bean></beans>");
    quiet("<project><bean><property name=\"n\" value=\"5\"/></bean></project>");
    let text = beans_xml("<bean id=\"a\" class=\"x.Y\"><property name=\"n\" value=\"5\"/></bean>");
    assert!(xml_actions::use_p_namespace(&text, 0).is_none());
}

const INNER: &str = "<bean id=\"a\" class=\"x.Y\">\n  <property name=\"dao\">\n    <bean class=\"com.acme.UserDao\">\n      <property name=\"k\" value=\"1\"/>\n    </bean>\n  </property>\n</bean>";

#[test]
fn introduce_bean_moves_inner_bean_and_references_it() {
    let text = beans_xml(INNER);
    let edits = xml_actions::introduce_bean(&text, caret_in(&text, "<bean class")).unwrap();
    let fixed = apply_edits(&text, &edits);
    let els = xml::elements(&fixed);
    let names: Vec<_> = els.iter().map(|e| (e.name.as_str(), e.parent)).collect();
    assert_eq!(names, [("beans", None), ("bean", Some(0)), ("property", Some(1)), ("bean", Some(0)), ("property", Some(3))]);
    assert_eq!(els[2].value("ref"), Some("userDao"));
    assert_eq!(els[3].value("id"), Some("userDao"));
    assert_eq!(els[3].value("class"), Some("com.acme.UserDao"));
    assert!(fixed.contains("</bean>\n<bean id=\"userDao\" class=\"com.acme.UserDao\">\n  <property name=\"k\" value=\"1\"/>\n</bean>\n</beans>"), "{fixed}");

    let list = beans_xml("<bean id=\"a\" class=\"x.Y\"><property name=\"l\"><list><bean class=\"p.Q$R\"/></list></property></bean>");
    let fixed = apply_edits(&list, &xml_actions::introduce_bean(&list, caret_in(&list, "<bean class")).unwrap());
    let els = xml::elements(&fixed);
    let reference = els.iter().find(|e| e.name == "ref").unwrap();
    assert_eq!(reference.value("bean"), Some("r"));
    assert_eq!(els[reference.parent.unwrap()].name, "list");
    assert_eq!(els.iter().find(|e| e.value("id") == Some("r")).unwrap().parent, Some(0));
}

#[test]
fn introduce_bean_picks_an_unused_id() {
    let text = beans_xml("<bean id=\"userDao\" class=\"a.B\"/><bean name=\"x, userDao2\" class=\"a.C\"/>\n<alias name=\"userDao\" alias=\"userDao3\"/>\n<bean id=\"a\" class=\"x.Y\"><constructor-arg><bean class=\"com.acme.UserDao\"/></constructor-arg></bean>");
    let fixed = apply_edits(&text, &xml_actions::introduce_bean(&text, caret_in(&text, "<bean class")).unwrap());
    let els = xml::elements(&fixed);
    assert!(els.iter().any(|e| e.name == "bean" && e.value("id") == Some("userDao4")), "{fixed}");
    let arg = els.iter().find(|e| e.name == "constructor-arg").unwrap();
    assert_eq!(arg.value("ref"), Some("userDao4"));
}

#[test]
fn introduce_bean_is_quiet_when_bean_has_id() {
    let quiet = |text: &str| assert!(xml_actions::introduce_bean(text, text.rfind("<bean").unwrap() + 2).is_none(), "{text}");
    quiet(&beans_xml("<bean id=\"a\" class=\"x.Y\"><property name=\"d\"><bean class=\"c.D\" id=\"d\"/></property></bean>"));
    let top = beans_xml("<bean class=\"c.D\"/>");
    assert!(xml_actions::introduce_bean(&top, caret_in(&top, "<bean class")).is_none());
    quiet(&beans_xml("<bean id=\"a\"><property name=\"d\"><bean class=\"${c}\"/></property></bean>"));
    quiet(&beans_xml("<bean id=\"a\"><property name=\"d\"><bean parent=\"p\"/></property></bean>"));
    quiet(&beans_xml("<bean id=\"a\"><property name=\"d\"><bean class=\"c.D\">"));
}

const DTD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE beans PUBLIC \"-//SPRING//DTD BEAN 2.0//EN\" \"https://www.springframework.org/dtd/spring-beans-2.0.dtd\">\n<beans default-lazy-init=\"true\">\n  <bean id=\"a\" class=\"x.Y\"/>\n</beans>\n";

#[test]
fn update_schema_replaces_doctype() {
    let fixed = apply_edits(DTD, &xml_actions::update_schema(DTD).unwrap());
    assert!(!fixed.contains("DOCTYPE"), "{fixed}");
    assert!(fixed.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<beans xmlns="), "{fixed}");
    let els = xml::elements(&fixed);
    assert_eq!(xml::namespace(&els, 0), Some("http://www.springframework.org/schema/beans"));
    assert_eq!(els[0].value("xsi:schemaLocation"), Some("http://www.springframework.org/schema/beans https://www.springframework.org/schema/beans/spring-beans.xsd"));
    assert_eq!(els[0].value("xmlns:xsi"), Some("http://www.w3.org/2001/XMLSchema-instance"));
    assert_eq!(els[0].value("default-lazy-init"), Some("true"));
    assert_eq!(els.len(), 2);
    assert_eq!(xml::namespace(&els, 1), Some("http://www.springframework.org/schema/beans"));
}

#[test]
fn update_schema_is_quiet_for_schema_files() {
    assert!(xml_actions::update_schema(&beans_xml("<bean id=\"a\" class=\"x.Y\"/>")).is_none());
    let both = DTD.replace("<beans ", "<beans xmlns=\"http://www.springframework.org/schema/beans\" ");
    assert!(xml_actions::update_schema(&both).is_none());
    assert!(xml_actions::update_schema(&DTD.replace("beans PUBLIC", "html PUBLIC").replace("<beans ", "<html ").replace("</beans>", "</html>")).is_none());
    assert!(xml_actions::update_schema("<project><!DOCTYPE beans></project>").is_none());
}

#[test]
fn intentions_keep_multibyte_text_and_crlf() {
    let crlf = |s: &str| s.replace('\n', "\r\n");
    let text = crlf(&beans_xml("<!-- \u{1F600} -->\n<bean id=\"a\" class=\"x.Y\">\n  <property name=\"n\" value=\"caf\u{e9} \u{1F600}\"/>\n  <property name=\"dao\">\n    <bean class=\"com.acme.UserDao\">\n      <property name=\"k\" value=\"\u{e9}\"/>\n    </bean>\n  </property>\n</bean>"));
    let fixed = apply_edits(&text, &xml_actions::use_p_namespace(&text, caret_in(&text, "<property name=\"n\"")).unwrap());
    let els = xml::elements(&fixed);
    assert_eq!(els.iter().find(|e| e.name == "bean").unwrap().value("p:n"), Some("caf\u{e9} \u{1F600}"));
    assert!(!fixed.contains("\r\n\r\n") && !fixed.replace("\r\n", "").contains('\n'), "{fixed:?}");

    let fixed = apply_edits(&text, &xml_actions::introduce_bean(&text, caret_in(&text, "<bean class")).unwrap());
    let els = xml::elements(&fixed);
    assert_eq!(els.iter().find(|e| e.name == "property" && e.value("name") == Some("dao")).unwrap().value("ref"), Some("userDao"));
    assert_eq!(els.iter().find(|e| e.value("id") == Some("userDao")).unwrap().parent, Some(0));
    assert!(fixed.contains("\r\n<bean id=\"userDao\" class=\"com.acme.UserDao\">\r\n  <property name=\"k\" value=\"\u{e9}\"/>\r\n</bean>\r\n"), "{fixed:?}");
    assert!(!fixed.replace("\r\n", "").contains('\n'));

    let dtd = crlf(&DTD.replace("x.Y", "caf\u{e9}\u{1F600}"));
    let fixed = apply_edits(&dtd, &xml_actions::update_schema(&dtd).unwrap());
    assert!(fixed.contains("<beans xmlns=\"http://www.springframework.org/schema/beans\"\r\n       xmlns:xsi"), "{fixed:?}");
    assert!(!fixed.replace("\r\n", "").contains('\n'));
    assert_eq!(xml::elements(&fixed)[1].value("class"), Some("caf\u{e9}\u{1F600}"));
}

#[test]
fn xml_intentions_are_offered_as_code_actions() {
    let text = beans_xml("<bean id=\"a\" class=\"x.Y\"><property name=\"n\" value=\"5\"/><property name=\"d\"><bean class=\"c.D\"/></property></bean>");
    let document = doc(XML_PATH, &text);
    let mut navigator = Navigator::default();
    let at = |needle: &str| {
        let p = position(&text, caret_in(&text, needle));
        Range::new(p, p)
    };
    let mut titles = |range| navigator.code_actions(&document, range).into_iter().map(|a| a.title).collect::<Vec<_>>();
    assert_eq!(titles(at("<property name=\"n\"")), ["Use p-namespace"]);
    assert_eq!(titles(at("<bean class")), ["Introduce bean"]);
    let dtd = doc(XML_PATH, DTD);
    let first = Range::new(Position::new(0, 0), Position::new(0, 0));
    assert_eq!(navigator.code_actions(&dtd, first).iter().map(|a| a.title.as_str()).collect::<Vec<_>>(), ["Update XML schema"]);
}

#[test]
fn style_fix_handles_unusual_parent_tags() {
    let fix = |body: &str| {
        let text = beans_xml(body);
        let els = xml::elements(&text);
        let property = els.iter().find(|e| e.local() == "property").unwrap();
        apply_edits(&text, &[xml_checks::attribute_style_edit(&text, property, &els).unwrap()])
    };
    let fixed = fix("<bean class=\"x.Y\"><property name=\"a&gt;b\" title=\"x>y\"><value>v</value></property></bean>");
    let els = xml::elements(&fixed);
    assert_eq!(els.iter().find(|e| e.name == "property").unwrap().value("value"), Some("v"));
    assert_eq!(els.iter().find(|e| e.name == "property").unwrap().value("title"), Some("x>y"));
    assert!(els.iter().all(|e| e.name != "value"));

    let fixed = fix("<b:bean xmlns:b=\"http://www.springframework.org/schema/beans\"><b:property name=\"n\"><b:ref bean=\"o\"/></b:property></b:bean>");
    let els = xml::elements(&fixed);
    assert_eq!(els.iter().find(|e| e.name == "b:property").unwrap().value("ref"), Some("o"));
    assert!(els.iter().all(|e| e.name != "b:ref"));

    let fixed = fix("<bean class=\"x.Y\"><property name='n'><value>v</value></property></bean>");
    let els = xml::elements(&fixed);
    let property = els.iter().find(|e| e.name == "property").unwrap();
    assert_eq!((property.value("name"), property.value("value")), (Some("n"), Some("v")));
    assert!(els.iter().all(|e| e.name != "value"));

    let text = "<beans xmlns=\"http://www.springframework.org/schema/beans\"><bean><property name=\"n\"";
    let els = xml::elements(text);
    assert!(xml_checks::attribute_style_edit(text, &els[2], &els).is_none());
}

#[test]
fn autowired_static_field_is_reported() {
    let text = "@Service\nclass A {\n    @Autowired\n    private static Repo repo;\n}\n";
    assert_eq!(found(text), [pair("static-injection", "repo")]);
    let constant = "@Service\nclass A {\n    @Value(\"${a}\")\n    static final String NAME = null;\n}\n";
    assert_eq!(found(constant), [pair("static-injection", "NAME")]);
}

#[test]
fn value_spel_on_static_field_reports_the_spel_code_only() {
    let text = "@Service\nclass A {\n    @Value(\"#{systemProperties['x']}\")\n    static String x;\n}\n";
    assert_eq!(found(text), [pair("static-spel-injection", "x")]);
}

#[test]
fn inject_on_static_method_is_reported() {
    let text = "import jakarta.inject.Inject;\n\n@Service\nclass A {\n    @Inject\n    static void setRepo(Repo r) {}\n}\n";
    assert_eq!(found(text), [pair("static-injection", "setRepo")]);
}

#[test]
fn two_required_autowired_constructors_are_reported() {
    let text = "@Service\nclass A {\n    @Autowired\n    A(X x) {}\n    @Autowired(required = true)\n    A(Y y) {}\n}\n";
    assert_eq!(found(text), [pair("autowired-constructors", "A")]);
    let host = MockHost::install();
    let document = java("Subject.java", text);
    Navigator::default().line_markers(&document);
    assert_eq!(host.diagnostics()[&document.path][0].range.start.line, 5);
}

#[test]
fn required_next_to_optional_constructor_is_reported() {
    let text = "@Service\nclass A {\n    @Autowired\n    A(X x) {}\n    @Autowired(required = false)\n    A(Y y) {}\n}\n";
    assert_eq!(found(text), [pair("autowired-constructors", "A")]);
}

#[test]
fn optional_autowired_constructors_are_quiet() {
    let text = "@Service\nclass A {\n    @Autowired(required = false)\n    A(X x) {}\n    @Autowired(required = false)\n    A(Y y) {}\n}\n";
    assert!(found(text).is_empty());
}

#[test]
fn injection_checks_are_quiet_for_library_or_dynamic_name() {
    let foreign = "import com.acme.Autowired;\n\nclass A {\n    @Autowired\n    static Repo repo;\n    @Autowired\n    A(X x) {}\n    @Autowired\n    A(Y y) {}\n}\n";
    assert!(found(foreign).is_empty());
    let dynamic = "class A {\n    @Autowired(required = FLAG)\n    A(X x) {}\n    @Autowired\n    A(Y y) {}\n}\n";
    assert!(found(dynamic).is_empty());
    let lombok = "@RequiredArgsConstructor\n@Service\nclass A {\n    private final Repo repo;\n}\n";
    assert!(found(lombok).is_empty());
    let record = "record A(@Autowired static int x) {\n    @Autowired static Repo repo;\n}\n";
    assert!(found(record).is_empty());
    let instance = "@Service\nclass A {\n    @Autowired\n    private Repo repo;\n    @Autowired\n    void set(Repo r) {}\n    @Value(\"#{x}\")\n    String y;\n}\n";
    assert!(found(instance).is_empty());
}

#[test]
fn switched_off_injection_group_is_quiet() {
    let host = MockHost::install();
    host.set_setting("springBeans.checkInjectionAnnotations", "false");
    let text = "@Service\nclass A {\n    @Autowired\n    static Repo repo;\n}\n";
    assert!(findings(&host, &mut Navigator::default(), "Subject.java", text).is_empty());
}

#[test]
fn injection_ranges_use_utf16_columns_with_multibyte_text() {
    let text = "@Service\nclass A {\n    @Qualifier(\"é\") @Autowired static Repo repo;\n}\n";
    let host = MockHost::install();
    let document = java("Subject.java", text);
    Navigator::default().line_markers(&document);
    let diagnostics = &host.diagnostics()[&document.path];
    let diagnostic = diagnostics.iter().find(|d| d.code.as_deref() == Some("static-injection")).unwrap();
    assert_eq!((diagnostic.range.start.line, diagnostic.range.start.character), (2, 43));
}

#[test]
fn profile_expression_problem_cases() {
    for bad in ["", "  ", "a & b | c", "a | b & c", "!", "(a & b", "a & b)", "!!", "()"] {
        assert!(annotation_checks::profile_expression_problem(bad).is_some(), "{bad:?} should report");
    }
    for good in ["a | b", "!dev", "(a & b) | c", "a", " a & b ", "!(a | b)", "a & (b | c)", "((a))", "a&b&c", "!a & !b", "a &", "a |", "a && b", "a !b", "(a)(b)"] {
        assert_eq!(annotation_checks::profile_expression_problem(good), None, "{good:?} should be quiet");
    }
}

#[test]
fn empty_profile_is_reported() {
    let text = "@Profile(\"\")\n@Service\nclass A {\n    @Bean\n    @Profile(\" \")\n    B b() { return null; }\n}\n";
    assert_eq!(found(text), [pair("profile-expression", ""), pair("profile-expression", " ")]);
}

#[test]
fn mixed_operators_are_reported() {
    let text = "@Service\nclass A {\n    @Profile({\"x\", \"a & b | c\", \"!\"})\n    void m() {}\n}\n";
    assert_eq!(found(text), [pair("profile-expression", "a & b | c"), pair("profile-expression", "!")]);
    let valid = "@Profile({\"a\", \"!b\", \"(a & b) | c\"})\n@Service\nclass A {}\n";
    assert!(found(valid).is_empty());
}

#[test]
fn profile_checks_are_quiet_for_library_or_dynamic_name() {
    let constant = "@Profile(NAMES)\n@Service\nclass A {}\n";
    assert!(found(constant).is_empty());
    let concatenated = "@Profile(\"a &\" + X)\n@Service\nclass A {}\n";
    assert!(found(concatenated).is_empty());
    let placeholder = "@Profile(\"${p} & !\")\n@Service\nclass A {}\n";
    assert!(found(placeholder).is_empty());
    let foreign = "import com.acme.Profile;\n\n@Profile(\"\")\n@Service\nclass A {}\n";
    assert!(found(foreign).is_empty());
}

#[test]
fn profile_ranges_use_utf16_columns_with_multibyte_text() {
    let text = "@Service\nclass A {\n    @Named(\"é\") @Profile(\"a & b | c\") void m() {}\n}\n";
    let host = MockHost::install();
    let document = java("Subject.java", text);
    Navigator::default().line_markers(&document);
    let diagnostic = host.diagnostics()[&document.path].iter().find(|d| d.code.as_deref() == Some("profile-expression")).unwrap().clone();
    assert_eq!((diagnostic.range.start.line, diagnostic.range.start.character), (2, 26));
}

#[test]
fn async_returning_string_is_reported() {
    let text = "@Service\nclass A {\n    @Async\n    public String a() { return null; }\n    @Async\n    int b() { return 0; }\n    @Async\n    public List<String> c() { return null; }\n}\n";
    assert_eq!(found(text), [pair("async-return-type", "String"), pair("async-return-type", "int"), pair("async-return-type", "List")]);
}

#[test]
fn async_returning_future_types_is_quiet() {
    let text = "@Service\nclass A {\n    @Async\n    void a() {}\n    @Async\n    Future<String> b() { return null; }\n    @Async\n    CompletableFuture<String> c() { return null; }\n    @Async\n    ListenableFuture<String> d() { return null; }\n    @Async\n    CompletionStage<String> e() { return null; }\n}\n";
    assert!(found(text).is_empty());
}

#[test]
fn async_check_is_quiet_for_library_or_dynamic_name() {
    let project = "@Service\nclass A {\n    @Async\n    Result a() { return null; }\n}\n";
    assert!(found(project).is_empty());
    let foreign = "import com.acme.Async;\n\n@Service\nclass A {\n    @Async\n    String a() { return null; }\n}\n";
    assert!(found(foreign).is_empty());
    let plain = "@Service\nclass A {\n    String a() { return null; }\n}\n";
    assert!(found(plain).is_empty());
}

#[test]
fn switched_off_profile_and_async_groups_are_quiet() {
    let text = "@Profile(\"\")\n@Service\nclass A {\n    @Async\n    String a() { return null; }\n}\n";
    let host = MockHost::install();
    host.set_setting("springBeans.checkProfiles", "false");
    assert_eq!(findings(&host, &mut Navigator::default(), "Subject.java", text), [pair("async-return-type", "String")]);
    host.set_setting("springBeans.checkAsync", "false");
    assert!(findings(&host, &mut Navigator::default(), "Subject.java", text).is_empty());
}

#[test]
fn async_ranges_use_utf16_columns_with_multibyte_text() {
    let text = "@Service\nclass A {\n    @Named(\"é\") @Async String a() { return null; }\n}\n";
    let host = MockHost::install();
    let document = java("Subject.java", text);
    Navigator::default().line_markers(&document);
    let diagnostic = host.diagnostics()[&document.path].iter().find(|d| d.code.as_deref() == Some("async-return-type")).unwrap().clone();
    assert_eq!((diagnostic.range.start.line, diagnostic.range.start.character), (2, 23));
}

#[test]
fn cacheable_without_cache_name_is_reported() {
    let text = "@Service\nclass A {\n    @Cacheable\n    String a() { return null; }\n    @CachePut(key = \"#id\")\n    String b() { return null; }\n    @CacheEvict(allEntries = true)\n    void c() {}\n    @Cacheable(\"x\")\n    String d() { return null; }\n}\n";
    assert_eq!(
        found(text),
        [
            pair("cache-names-missing", "@Cacheable"),
            pair("cache-names-missing", "@CachePut(key = \"#id\")"),
            pair("cache-names-missing", "@CacheEvict(allEntries = true)"),
        ]
    );
}

#[test]
fn cache_config_on_class_supplies_the_name() {
    let text = "@CacheConfig(cacheNames = \"x\")\n@Service\nclass A {\n    @Cacheable\n    String a() { return null; }\n}\n";
    assert!(found(text).is_empty());
}

#[test]
fn cache_annotation_on_interface_method_is_reported() {
    let text = "@Cacheable(\"t\")\ninterface A {\n    @Cacheable(\"x\")\n    String a();\n}\n";
    assert_eq!(found(text), [pair("cache-annotation-on-interface", "@Cacheable(\"t\")"), pair("cache-annotation-on-interface", "@Cacheable(\"x\")")]);
}

#[test]
fn cacheable_and_cache_put_together_are_reported() {
    let text = "@Service\nclass A {\n    @Cacheable(\"x\") @CachePut(\"x\")\n    String a() { return null; }\n}\n";
    assert_eq!(found(text), [pair("cacheable-and-cache-put", "@CachePut(\"x\")")]);
}

#[test]
fn cache_checks_are_quiet_for_library_or_dynamic_name() {
    let constant = "@Service\nclass A {\n    @Cacheable(NAME)\n    String a() { return null; }\n    @Cacheable(cacheNames = PREFIX + \"x\")\n    String b() { return null; }\n}\n";
    assert!(found(constant).is_empty());
    let foreign = "import com.acme.Cacheable;\n\n@Service\nclass A {\n    @Cacheable\n    String a() { return null; }\n}\n\ninterface B {\n    @Cacheable\n    String a();\n}\n";
    assert!(found(foreign).is_empty());
    let caching = "@Service\nclass A {\n    @Caching(cacheable = @Cacheable(\"x\"), put = @CachePut(\"x\"))\n    String a() { return null; }\n}\n";
    assert!(found(caching).is_empty());
    let dynamic_config = "@CacheConfig(cacheNames = NAMES)\n@Service\nclass A {\n    @Cacheable\n    String a() { return null; }\n}\n";
    assert!(found(dynamic_config).is_empty());
    let spring = "import org.springframework.cache.annotation.Cacheable;\n\n@Service\nclass A {\n    @Cacheable\n    String a() { return null; }\n}\n";
    assert_eq!(found(spring), [pair("cache-names-missing", "@Cacheable")]);
}

#[test]
fn switched_off_cache_group_is_quiet() {
    let text = "@Service\nclass A {\n    @Cacheable\n    String a() { return null; }\n}\n";
    let host = MockHost::install();
    assert_eq!(findings(&host, &mut Navigator::default(), "Subject.java", text).len(), 1);
    host.set_setting("springBeans.checkCache", "false");
    assert!(findings(&host, &mut Navigator::default(), "Subject.java", text).is_empty());
}

#[test]
fn cache_ranges_use_utf16_columns_with_multibyte_text() {
    let text = "@Service\nclass A {\n    @Named(\"é\") @Cacheable String a() { return null; }\n}\n";
    let host = MockHost::install();
    let document = java("Subject.java", text);
    Navigator::default().line_markers(&document);
    let diagnostic = host.diagnostics()[&document.path].iter().find(|d| d.code.as_deref() == Some("cache-names-missing")).unwrap().clone();
    assert_eq!((diagnostic.range.start.line, diagnostic.range.start.character), (2, 16));
}

const KT_TX: &str = "@Service\nclass Billing {\n    @Transactional\n    fun charge() {}\n}\n";
const KTS_BUILD: &str = "plugins {\n    kotlin(\"jvm\") version \"1.9.24\"\n    id(\"org.springframework.boot\") version \"3.3.0\"\n}\n";
const KT_PATH: &str = "/project/src/main/kotlin/Billing.kt";

fn kt_host(build: Option<(&str, &str)>) -> MockHost {
    let host = MockHost::install();
    if let Some((name, text)) = build {
        host.add_file(name, text);
    }
    host
}

fn kt_findings(host: &MockHost, text: &str) -> Vec<(String, String)> {
    let document = doc(KT_PATH, text);
    Navigator::default().line_markers(&document);
    host.diagnostics()
        .get(&document.path)
        .into_iter()
        .flatten()
        .filter(|d| d.code.as_deref() == Some("kotlin-spring-plugin"))
        .map(|d| {
            let (from, to) = (offset(text, d.range.start).unwrap(), offset(text, d.range.end).unwrap());
            (d.code.clone().unwrap(), text[from..to].to_owned())
        })
        .collect()
}

#[test]
fn final_transactional_kotlin_class_without_plugin_is_reported() {
    let host = kt_host(Some(("build.gradle.kts", KTS_BUILD)));
    assert_eq!(kt_findings(&host, KT_TX), [pair("kotlin-spring-plugin", "Billing")]);
    let on_class = "@Transactional\nclass Billing\n";
    assert_eq!(kt_findings(&host, on_class), [pair("kotlin-spring-plugin", "Billing")]);
}

#[test]
fn configuration_class_without_plugin_is_reported() {
    let host = kt_host(Some(("pom.xml", "<project/>")));
    assert_eq!(kt_findings(&host, "@Configuration\nclass Config\n"), [pair("kotlin-spring-plugin", "Config")]);
    let explicit = "@Configuration(proxyBeanMethods = true)\nclass Config\n";
    assert_eq!(kt_findings(&host, explicit).len(), 1);
}

#[test]
fn plugin_in_build_file_silences_the_check() {
    for build in [
        "plugins { kotlin(\"plugin.spring\") version \"1.9.24\" }",
        "plugins { id 'org.jetbrains.kotlin.plugin.spring' version '1.9.24' }",
        "apply plugin: 'kotlin-spring'",
        "<compilerPlugins>\n  <plugin>spring</plugin>\n</compilerPlugins>",
        "compilerPlugins { \"spring\" }",
    ] {
        let host = kt_host(Some(("build.gradle.kts", build)));
        assert!(kt_findings(&host, KT_TX).is_empty(), "{build}");
    }
    let host = kt_host(Some(("build.gradle.kts", "compilerPlugins { \"jpa\" }")));
    assert_eq!(kt_findings(&host, KT_TX).len(), 1);
}

#[test]
fn open_or_abstract_class_is_quiet() {
    let host = kt_host(Some(("build.gradle.kts", KTS_BUILD)));
    for decl in [
        "open class",
        "abstract class",
        "sealed class",
        "data class",
        "enum class",
        "annotation class",
        "interface",
        "object",
    ] {
        let text = format!("@Transactional\n{decl} Billing\n");
        assert!(kt_findings(&host, &text).is_empty(), "{decl}");
    }
    let inner = "class Outer {\n    @Transactional\n    inner class Billing\n}\n";
    assert!(kt_findings(&host, inner).is_empty());
}

#[test]
fn kotlin_spring_plugin_is_quiet_for_library_or_dynamic_name() {
    assert!(kt_findings(&kt_host(None), KT_TX).is_empty());
    let host = kt_host(Some(("build.gradle.kts", KTS_BUILD)));
    let foreign = "import com.acme.Transactional\n\nclass Billing {\n    @Transactional\n    fun charge() {}\n}\n";
    assert!(kt_findings(&host, foreign).is_empty());
    assert!(kt_findings(&host, "class Billing {\n    @Cacheable\n    fun a() {}\n}\n").len() == 1);
    assert!(kt_findings(&host, "@Configuration(proxyBeanMethods = false)\nclass Config\n").is_empty());
    assert!(kt_findings(&host, "@Configuration(proxyBeanMethods = FLAG)\nclass Config\n").is_empty());
    assert!(kt_findings(&host, "class Plain {\n    fun a() {}\n}\n").is_empty());
    let allopen = kt_host(Some(("build.gradle", "plugins { id 'org.jetbrains.kotlin.plugin.allopen' }")));
    assert!(kt_findings(&allopen, KT_TX).is_empty());
}

fn edited(text: &str, edit: &TextEdit) -> String {
    let (from, to) = (offset(text, edit.range.start).unwrap(), offset(text, edit.range.end).unwrap());
    format!("{}{}{}", &text[..from], edit.text, &text[to..])
}

#[test]
fn gradle_kts_fix_inserts_plugin_line() {
    let edit = kotlin_build::gradle_fix("/project/build.gradle.kts", KTS_BUILD).unwrap();
    assert_eq!(
        edited(KTS_BUILD, &edit),
        "plugins {\n    kotlin(\"jvm\") version \"1.9.24\"\n    id(\"org.springframework.boot\") version \"3.3.0\"\n    kotlin(\"plugin.spring\") version \"1.9.24\"\n}\n"
    );
    let host = kt_host(Some(("build.gradle.kts", KTS_BUILD)));
    let document = doc(KT_PATH, KT_TX);
    let actions = Navigator::default().code_actions(&document, Range::new(Position::new(1, 8), Position::new(1, 8)));
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].edits[0].0, "/project/build.gradle.kts");
    assert_eq!(edited(KTS_BUILD, &actions[0].edits[0].1[0]), edited(KTS_BUILD, &edit));
    assert!(Navigator::default().code_actions(&document, Range::new(Position::new(3, 5), Position::new(3, 5))).is_empty());
    drop(host);
}

#[test]
fn groovy_gradle_fix_inserts_plugin_line() {
    let build = "plugins {\n  id 'org.jetbrains.kotlin.jvm' version '1.9.24'\n}\n";
    let edit = kotlin_build::gradle_fix("/project/build.gradle", build).unwrap();
    assert_eq!(
        edited(build, &edit),
        "plugins {\n  id 'org.jetbrains.kotlin.jvm' version '1.9.24'\n  id 'org.jetbrains.kotlin.plugin.spring' version '1.9.24'\n}\n"
    );
    let crlf = build.replace('\n', "\r\n");
    let edit = kotlin_build::gradle_fix("/project/build.gradle", &crlf).unwrap();
    assert!(edited(&crlf, &edit).contains("'1.9.24'\r\n  id 'org.jetbrains.kotlin.plugin.spring' version '1.9.24'\r\n}"));
}

#[test]
fn maven_build_offers_no_fix() {
    let host = kt_host(Some(("pom.xml", "<project/>")));
    let document = doc(KT_PATH, KT_TX);
    assert!(Navigator::default().code_actions(&document, Range::new(Position::new(1, 8), Position::new(1, 8))).is_empty());
    assert_eq!(kt_findings(&host, KT_TX).len(), 1);
    assert!(kotlin_build::gradle_fix("/project/pom.xml", "<project/>").is_none());
    assert!(kotlin_build::gradle_fix("/project/build.gradle.kts", "dependencies {}\n").is_none());
    assert!(kotlin_build::gradle_fix("/project/build.gradle.kts", "plugins {\n    java\n}\n").is_none());
    assert!(kotlin_build::gradle_fix("/project/build.gradle.kts", "plugins {\n    kotlin(\"jvm\") version kv\n}\n").is_none());
    assert!(kotlin_build::gradle_fix("/project/build.gradle.kts", "plugins { kotlin(\"jvm\") version \"1.9\" }\n").is_none());
}

#[test]
fn fix_never_edits_outside_the_project() {
    let host = kt_host(Some(("build.gradle.kts", KTS_BUILD)));
    host.add_file("../outside/build.gradle.kts", KTS_BUILD);
    for path in ["/project/../outside/src/Billing.kt", "/projectile/src/Billing.kt", "/etc/Billing.kt"] {
        let document = doc(path, KT_TX);
        assert!(kotlin_build::fix_for(path).is_none(), "{path}");
        assert!(Navigator::default().code_actions(&document, Range::new(Position::new(1, 8), Position::new(1, 8))).is_empty(), "{path}");
    }
    assert!(kotlin_build::fix_for(KT_PATH).is_some());
}

#[test]
fn switched_off_kotlin_plugin_group_is_quiet() {
    let host = kt_host(Some(("build.gradle.kts", KTS_BUILD)));
    assert_eq!(kt_findings(&host, KT_TX).len(), 1);
    host.set_setting("springBeans.checkKotlinSpringPlugin", "false");
    assert!(kt_findings(&host, KT_TX).is_empty());
    let document = doc(KT_PATH, KT_TX);
    assert!(Navigator::default().code_actions(&document, Range::new(Position::new(1, 8), Position::new(1, 8))).is_empty());
}

#[test]
fn fixed_kotlin_file_publishes_empty_list() {
    let host = kt_host(Some(("build.gradle.kts", KTS_BUILD)));
    let mut navigator = Navigator::default();
    navigator.line_markers(&doc(KT_PATH, KT_TX));
    assert!(host.diagnostics()[KT_PATH].iter().any(|d| d.code.as_deref() == Some("kotlin-spring-plugin")));
    navigator.line_markers(&doc(KT_PATH, "open class Billing\n"));
    assert!(host.diagnostics().get(KT_PATH).is_none_or(|list| list.is_empty()));
}

#[test]
fn kotlin_plugin_ranges_use_utf16_columns_with_multibyte_text() {
    let text = "@Service\n@Named(\"é😀\") class Billing {\n    @Transactional fun a() {}\n}\n";
    let host = kt_host(Some(("build.gradle.kts", KTS_BUILD)));
    let document = doc(KT_PATH, text);
    Navigator::default().line_markers(&document);
    let d = host.diagnostics()[KT_PATH].iter().find(|d| d.code.as_deref() == Some("kotlin-spring-plugin")).unwrap().clone();
    assert_eq!((d.range.start.line, d.range.start.character, d.range.end.character), (1, 20, 27));
}

#[test]
fn kotlin_plugin_crlf_files_report_correct_lines() {
    let host = kt_host(Some(("build.gradle.kts", KTS_BUILD)));
    let text = KT_TX.replace('\n', "\r\n");
    let document = doc(KT_PATH, &text);
    Navigator::default().line_markers(&document);
    let d = host.diagnostics()[KT_PATH].iter().find(|d| d.code.as_deref() == Some("kotlin-spring-plugin")).unwrap().clone();
    assert_eq!((d.range.start.line, d.range.start.character), (1, 6));
}
