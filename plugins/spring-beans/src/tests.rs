use super::*;
use openidea_plugin::Event;
use openidea_plugin::mock::MockHost;

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
