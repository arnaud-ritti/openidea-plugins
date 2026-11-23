use crate::{OWNER_WINDOW, WINDOW_SECTION, WebTools};
use cloud::CloudTools;
use graphql::GraphQlTools;
use messaging::Messaging;
use openidea_plugin::providers::{CodeAction, CompletionItem, Document, LineMarker, Providers};
use openidea_plugin::views::{View, ViewEvent, Viewport, Views};
use openidea_plugin::{Error, Event, Invocation, Location, Plugin, Position, Range, host};
use spring_text::index::project_of;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use ui_kit::sections::{SectionDef, Switcher};
use webflow::WebFlow;
use websocket::WebSocketTools;

const SECTIONS: &[SectionDef] = &[
    SectionDef { id: "http", label: "HTTP" },
    SectionDef { id: "websocket", label: "WebSocket" },
    SectionDef { id: "messaging", label: "Messaging" },
    SectionDef { id: "graphql", label: "GraphQL" },
    SectionDef { id: "webflow", label: "Web Flow" },
    SectionDef { id: "feign", label: "Feign" },
];
const SECTION_KEY: &str = "endpoints.section";
const APPLIES_TTL: Duration = Duration::from_secs(30);

const WEB_LANGUAGES: [&str; 6] = ["java", "kotlin", "xml", "http", "html", "jsp"];
const WEBSOCKET_LANGUAGES: [&str; 8] = ["java", "kotlin", "javascript", "javascriptreact", "typescript", "typescriptreact", "vue", "xml"];
const MESSAGING_LANGUAGES: [&str; 3] = ["java", "kotlin", "xml"];
const GRAPHQL_LANGUAGES: [&str; 3] = ["java", "kotlin", "graphql"];
const WEBFLOW_LANGUAGES: [&str; 1] = ["xml"];
const CLOUD_LANGUAGES: [&str; 4] = ["java", "kotlin", "yaml", "properties"];

#[derive(Clone, Copy, Default)]
struct Applies {
    web: bool,
    websocket: bool,
    messaging: bool,
    graphql: bool,
    webflow: bool,
    cloud: bool,
}

mod live;
mod path_actions;
#[cfg(test)]
mod feign_navigation_tests;

pub struct Endpoints {
    live: live::Live,
    path_actions:path_actions::State,
    web: WebTools,
    websocket: WebSocketTools,
    messaging: Messaging,
    graphql: GraphQlTools,
    subscriptions:crate::graphql_subscriptions::Runtime,
    webflow: WebFlow,
    cloud: CloudTools,
    sections: Switcher,
    applies: BTreeMap<String, (Instant, Applies)>,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            live: live::Live::default(),
            path_actions:path_actions::State::default(),
            web: WebTools::default(),
            websocket: WebSocketTools::default(),
            messaging: Messaging::default(),
            graphql: GraphQlTools::default(),
            subscriptions:Default::default(),
            webflow: WebFlow::default(),
            cloud: CloudTools::default(),
            sections: Switcher::new(SECTION_KEY, SECTIONS),
            applies: BTreeMap::new(),
        }
    }
}

impl Endpoints {
    fn applicable(&mut self, doc: &Document) -> Applies {
        let Some(project) = project_of(&doc.path) else {
            return Applies::default();
        };
        let root = project.root().unwrap_or_default();
        if let Some((at, applies)) = self.applies.get(&root)
            && at.elapsed() < APPLIES_TTL
        {
            return *applies;
        }

        let applies = Applies {
            web: crate::applies(&project),
            websocket: websocket::applies(&project),
            messaging: messaging::applies(&project),
            graphql: graphql::applies(&project),
            webflow: webflow::applies(&project),
            cloud: cloud::applies(&project),
        };
        self.applies.insert(root, (Instant::now(), applies));
        applies
    }

    fn wants_web(&mut self, doc: &Document) -> bool {
        WEB_LANGUAGES.contains(&doc.language.as_str()) && self.applicable(doc).web
    }

    fn wants_websocket(&mut self, doc: &Document) -> bool {
        WEBSOCKET_LANGUAGES.contains(&doc.language.as_str()) && self.applicable(doc).websocket
    }

    fn wants_messaging(&mut self, doc: &Document) -> bool {
        MESSAGING_LANGUAGES.contains(&doc.language.as_str()) && self.applicable(doc).messaging
    }

    fn wants_graphql(&mut self, doc: &Document) -> bool {
        GRAPHQL_LANGUAGES.contains(&doc.language.as_str()) && self.applicable(doc).graphql
    }

    fn wants_webflow(&mut self, doc: &Document) -> bool {
        WEBFLOW_LANGUAGES.contains(&doc.language.as_str()) && self.applicable(doc).webflow
    }

    fn wants_cloud(&mut self, doc: &Document) -> bool {
        CLOUD_LANGUAGES.contains(&doc.language.as_str()) && self.applicable(doc).cloud
    }
}

fn module_data(module: &str, action: CodeAction) -> CodeAction {
    CodeAction { data: action.data.map(|data| format!("{module}:{data}")), ..action }
}

impl Providers for Endpoints {
    fn completion(&mut self, doc: &Document, at: Position, trigger: Option<&str>) -> Vec<CompletionItem> {
        let mut items = Vec::new();
        if self.wants_web(doc) {
            items.extend(self.web.completion(doc, at, trigger));
        }
        if self.wants_websocket(doc) {
            items.extend(self.websocket.completion(doc, at, trigger));
        }
        if self.wants_messaging(doc) {
            items.extend(self.messaging.completion(doc, at, trigger));
        }
        if self.wants_graphql(doc) {
            items.extend(self.graphql.completion(doc, at, trigger));
        }
        if self.wants_webflow(doc) {
            items.extend(self.webflow.completion(doc, at, trigger));
        }
        if self.wants_cloud(doc) {
            items.extend(self.cloud.completion(doc, at, trigger));
        }
        items
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let mut found = Vec::new();
        if self.wants_web(doc) {
            found.extend(self.web.definition(doc, at));
        }
        if self.wants_websocket(doc) {
            found.extend(self.websocket.definition(doc, at));
        }
        if self.wants_messaging(doc) {
            found.extend(self.messaging.definition(doc, at));
        }
        if self.wants_graphql(doc) {
            found.extend(self.graphql.definition(doc, at));
        }
        if self.wants_webflow(doc) {
            found.extend(self.webflow.definition(doc, at));
        }
        if self.wants_cloud(doc) {
            found.extend(self.cloud.definition(doc, at));
        }
        found
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        if !self.wants_web(doc) {
            return Vec::new();
        }
        let actions = self.web.code_actions(doc, selection);
        actions.into_iter().map(|action| module_data(WINDOW_SECTION, action)).collect()
    }

    fn resolve_code_action(&mut self, doc: &Document, action: CodeAction) -> Result<CodeAction, Error> {
        let Some(data) = action.data.as_deref() else {
            return Ok(action);
        };
        let Some((module, rest)) = data.split_once(':') else {
            return Err(Error::Invalid(format!("code action {data}")));
        };
        let routed = CodeAction { data: Some(rest.to_owned()), ..action };
        match module {
            WINDOW_SECTION => self.web.resolve_code_action(doc, routed).map(|resolved| module_data(module, resolved)),
            _ => Err(Error::Invalid(format!("code action {data}"))),
        }
    }

    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let mut markers = Vec::new();
        if self.wants_web(doc) {
            markers.extend(self.web.line_markers(doc));
        }
        if self.wants_websocket(doc) {
            markers.extend(self.websocket.line_markers(doc));
        }
        if self.wants_messaging(doc) {
            markers.extend(self.messaging.line_markers(doc));
        }
        if self.wants_graphql(doc) {
            markers.extend(self.graphql.line_markers(doc));
        }
        if self.wants_webflow(doc) {
            self.webflow.active = self.sections.active() == "webflow";
            markers.extend(self.webflow.line_markers(doc));
        }
        if self.wants_cloud(doc) {
            markers.extend(self.cloud.line_markers(doc));
        }
        markers
    }
}

fn module_view(module: &str, name: &str) -> String {
    format!("{module}.{name}")
}

impl Views for Endpoints {
    fn render(&mut self, _view: &str, viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        (|| -> Result<View, Error> {
            let inner = match self.sections.active() {
                "websocket" => self.websocket.render(&module_view(websocket::WINDOW_SECTION, "destinations"), viewport, ctx)?,
                "messaging" => self.messaging.render(&module_view(messaging::WINDOW_SECTION, "messaging"), viewport, ctx)?,
                "graphql" => self.graphql.render(&module_view(graphql::WINDOW_SECTION, "endpoints"), viewport, ctx)?,
                "webflow" => self.webflow.render(&module_view(webflow::WINDOW_SECTION, "flow"), viewport, ctx)?,
                "feign" => self.cloud.render(&module_view(cloud::WINDOW_SECTION, "feign-clients"), viewport, ctx)?,
                _ => self.web.render(&module_view(WINDOW_SECTION, "endpoints"), viewport, ctx)?,
            };
            let mut view = self.sections.buttons(inner, &[]);
            let section = self.sections.active().to_owned();
            if let Some(project) = ctx.project()
                && let Some(note) = self.absence_note(&project, &section)
            {
                view.status = Some(match view.status {
                    Some(status) => format!("{status} · {note}"),
                    None => note,
                });
            }
            Ok(view)
        })().map(ui_kit::actions::available)
    }

    fn handle(&mut self, _view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if ui_kit::web::handle(self, _view, &event, ctx, include_str!("ui_config.json"))? {
            return Ok(());
        }

        if self.sections.handle(&event) {
            host::refresh_view(OWNER_WINDOW);
            return Ok(());
        }

        match self.sections.active() {
            "websocket" => self.websocket.handle(&module_view(websocket::WINDOW_SECTION, "destinations"), event, ctx),
            "messaging" => self.messaging.handle(&module_view(messaging::WINDOW_SECTION, "messaging"), event, ctx),
            "graphql" => self.graphql.handle(&module_view(graphql::WINDOW_SECTION, "endpoints"), event, ctx),
            "webflow" => self.webflow.handle(&module_view(webflow::WINDOW_SECTION, "flow"), event, ctx),
            "feign" => self.cloud.handle(&module_view(cloud::WINDOW_SECTION, "feign-clients"), event, ctx),
            _ => self.web.handle(&module_view(WINDOW_SECTION, "endpoints"), event, ctx),
        }
    }
}

impl Plugin for Endpoints {
    fn activate(&mut self, _ctx: &openidea_plugin::Context) -> Result<(), Error> {
        self.activate_live();
        Ok(())
    }
    fn run_command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        if matches!(id,crate::graphql_subscriptions::START|crate::graphql_subscriptions::POLL|crate::graphql_subscriptions::CANCEL){self.subscriptions.run_command(id,ctx)?;return Ok(None)}
        if id==crate::subscription_drafts::COMMAND {crate::subscription_drafts::run(&mut self.graphql,ctx,arg)?;return Ok(None)}
        if id==crate::source_lenses::GRAPHQL_COMMAND {crate::source_lenses::run_graphql(&mut self.graphql,ctx,arg)?;return Ok(None)}
        self.web.run_command(id, ctx, arg)
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
        self.live_events(events);
        if self.sections.active() == "webflow" {
            self.webflow.handle_events(events, ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::views::Body;

    fn inspect(host: &MockHost, endpoints: &mut Endpoints, doc: &Document) -> Vec<LineMarker> {
        endpoints.activate_live();
        host.open_editor(&doc.path, &doc.language, &doc.text, &[]);
        host.refresh_diagnostics(endpoints);
        assert!(host.diagnostics().is_empty());
        endpoints.line_markers(doc)
    }

    const JAVA_PATH: &str = "/project/src/main/java/com/acme/C.java";
    const JAVA: &str = "package com.acme;\n\n@Controller\nclass C {\n    @GetMapping(\"/u/{id}\")\n    String get(@PathVariable(\"uid\") Long id) { return \"\"; }\n\n    @QueryMapping\n    String nope() { return \"\"; }\n}\n";
    const SCHEMA: &str = "type Query {\n  books: [String]\n}\n";
    const WEB_POM: &str = "<project><artifactId>spring-boot-starter-web</artifactId></project>";
    const GRAPHQL_POM: &str =
        "<project><artifactId>spring-boot-starter-web</artifactId><artifactId>spring-boot-starter-graphql</artifactId></project>";
    const NO_SPRING_POM: &str = "<project><artifactId>commons-lang3</artifactId></project>";

    fn setup(pom: &str) -> MockHost {
        let host = MockHost::install();
        host.add_file("pom.xml", pom);
        host.add_file("src/main/java/com/acme/C.java", JAVA);
        host.add_file("src/main/resources/graphql/schema.graphqls", SCHEMA);
        host
    }

    fn doc(path: &str, language: &str, text: &str) -> Document {
        Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
    }

    fn codes(host: &MockHost) -> Vec<String> {
        host.provider_diagnostics().get(JAVA_PATH).into_iter().flatten().filter_map(|d| d.code.clone()).collect()
    }

    #[test]
    fn endpoints_published_with_unreadable_file() {
        use openidea_plugin::endpoints::EndpointsProvider;
        let host = setup(WEB_POM);
        host.add_file(
            "src/main/java/com/acme/Routes.java",
            "class Routes { RouterFunction<ServerResponse> r() { return RouterFunctions.route(RequestPredicates.GET(\"/r\"), this::list); } }",
        );
        let mut endpoints = Endpoints::default();
        endpoints.render("endpoints", None, &host.invocation()).unwrap();
        host.add_file_bytes("src/main/java/com/acme/Legacy.java", b"class caf\xe9 {}");
        endpoints.live_events(&[Event::FileSaved("/project/src/main/java/com/acme/Legacy.java".into())]);
        let published = endpoints.endpoints(&host.invocation()).unwrap();
        assert_eq!(published.len(), 1, "{published:?}");
        let view = endpoints.render("endpoints", None, &host.invocation()).unwrap();
        assert!(view.status.unwrap().contains("partial index (1 unreadable)"));
    }

    #[test]
    fn endpoints_fill_a_large_project_over_several_calls_without_failing() {
        use openidea_plugin::endpoints::EndpointsProvider;
        let host = setup(WEB_POM);
        let body = (0..40).map(|m| format!("    public String field{m}(String a, int b) {{ return a + b + \"{m}\"; }}\n")).collect::<String>();
        for n in 0..6_000 {
            let route = match n % 100 {
                0 => format!("    RouterFunction<ServerResponse> r() {{ return RouterFunctions.route(RequestPredicates.GET(\"/r{n}\"), this::list); }}\n"),
                _ => String::new(),
            };
            host.add_file(&format!("module{}/src/main/java/com/acme/m{n}/S{n}.java", n % 66), &format!("package com.acme.m{n};\n\npublic class S{n} {{\n{route}{body}}}\n"));
        }
        let mut endpoints = Endpoints::default();
        let (mut calls, mut published) = (0, 0);
        loop {
            calls += 1;
            let started = Instant::now();
            let found = endpoints.endpoints(&host.invocation()).expect("partial routes, not an error, while scanning");
            let took = started.elapsed();
            assert!(took < spring_text::index::VIEW_BUDGET + Duration::from_millis(200), "call {calls} took {took:?}");
            assert!(found.len() >= published, "call {calls} lost routes");
            published = found.len();
            if endpoints.web.endpoints.cached("/project/pom.xml").is_some_and(|index| !index.scanning()) {
                break;
            }
            assert!(calls < 1_000);
        }
        assert!(calls >= 2, "{calls} calls");
        assert_eq!(published, 60);
    }

    #[test]
    fn pom_line_markers_do_not_refresh_view() {
        let host = setup("<project><artifactId>spring-webflow</artifactId></project>");
        let mut endpoints = Endpoints::default();
        let pom = doc("/project/pom.xml", "xml", WEB_POM);
        endpoints.line_markers(&pom);
        assert!(host.refreshed_views().is_empty(), "{:?}", host.refreshed_views());
        let flow = "<flow xmlns=\"http://www.springframework.org/schema/webflow\"><view-state id=\"a\"/></flow>";
        let flow = doc("/project/flow.xml", "xml", flow);
        endpoints.line_markers(&flow);
        assert!(host.refreshed_views().is_empty(), "webflow section is not active");
        endpoints.handle("endpoints", ViewEvent::Button("section:webflow".into()), &host.invocation()).unwrap();
        let before = host.refreshed_views().len();
        endpoints.line_markers(&pom);
        assert_eq!(host.refreshed_views().len(), before);
        endpoints.line_markers(&flow);
        assert_eq!(host.refreshed_views().len(), before + 1);
    }

    #[test]
    fn diagnostics_on_a_cold_project_discover_no_files() {
        use openidea_plugin::diagnostics::DiagnosticsProvider;
        let _host = setup(GRAPHQL_POM);
        let mut endpoints = Endpoints::default();
        let all = Applies { web: true, websocket: true, messaging: true, graphql: true, webflow: true, cloud: true };
        endpoints.applies.insert("/project".into(), (Instant::now(), all));
        endpoints.diagnostics(&doc(JAVA_PATH, "java", JAVA));
        let discovered = |walked: bool, scanning: bool| walked || scanning;
        assert!(!endpoints.web.endpoints.cached(JAVA_PATH).is_some_and(|i| discovered(i.walked(), i.scanning())));
        assert!(!endpoints.web.versioning.cached(JAVA_PATH).is_some_and(|i| discovered(i.walked(), i.scanning())));
        assert!(endpoints.web.models.cached(JAVA_PATH).is_none());
    }

    #[test]
    fn providers_skip_modules_that_do_not_apply() {
        let host = setup(NO_SPRING_POM);
        let mut endpoints = Endpoints::default();
        inspect(&host, &mut endpoints, &doc(JAVA_PATH, "java", JAVA));
        assert!(host.provider_diagnostics().is_empty());

        let host = setup(GRAPHQL_POM);
        let mut endpoints = Endpoints::default();
        inspect(&host, &mut endpoints, &doc(JAVA_PATH, "java", JAVA));
        assert!(!host.provider_diagnostics().is_empty());
    }

    #[test]
    fn a_project_with_no_spring_dependency_does_no_module_work() {
        let host = MockHost::install();
        host.add_file("composer.json", "{}");
        host.add_file("src/Thing.java", "class Thing {}");
        let mut endpoints = Endpoints::default();
        let thing = doc("/project/src/Thing.java", "java", "class Thing {}");
        assert!(inspect(&host, &mut endpoints, &thing).is_empty());
        assert!(endpoints.completion(&thing, Position::new(0, 0), None).is_empty());
        assert!(endpoints.definition(&thing, Position::new(0, 0)).is_empty());
        assert!(endpoints.code_actions(&thing, Range::default()).is_empty());
        assert!(host.provider_diagnostics().is_empty());
    }

    #[test]
    fn one_failing_module_does_not_hide_the_others() {
        let host = setup(GRAPHQL_POM);
        host.add_file("src/main/resources/graphql/schema.graphqls", "{ not sdl");
        let mut endpoints = Endpoints::default();
        inspect(&host, &mut endpoints, &doc(JAVA_PATH, "java", JAVA));
        assert!(codes(&host).contains(&"unknown-path-variable".to_owned()), "{:?}", codes(&host));
    }

    #[test]
    fn diagnostics_of_every_module_are_published_together() {
        let host = setup(GRAPHQL_POM);
        let mut endpoints = Endpoints::default();
        inspect(&host, &mut endpoints, &doc(JAVA_PATH, "java", JAVA));
        let found = codes(&host);
        assert!(found.contains(&"unknown-path-variable".to_owned()), "{found:?}");
        assert!(found.contains(&"unknown-schema-field".to_owned()), "{found:?}");
    }

    #[test]
    fn code_action_resolves_through_its_module() {
        let _host = setup(WEB_POM);
        let mut endpoints = Endpoints::default();
        let java = doc(JAVA_PATH, "java", JAVA);
        let action = CodeAction { title: "t".into(), kind: None, preferred: false, edits: Vec::new(), data: Some("ref".into()) };

        let routed = CodeAction { data: Some(format!("{WINDOW_SECTION}:ref")), ..action.clone() };
        let resolved = endpoints.resolve_code_action(&java, routed).unwrap();
        assert_eq!(resolved.data.as_deref(), Some("web:ref"));

        let unknown = CodeAction { data: Some("nowhere:ref".into()), ..action };
        assert!(endpoints.resolve_code_action(&java, unknown).is_err());
    }

    #[test]
    fn section_buttons_switch_the_endpoints_window() {
        let host = setup(GRAPHQL_POM);
        let mut endpoints = Endpoints::default();
        let view = endpoints.render("endpoints", None, &host.invocation()).unwrap();
        let ids: Vec<_> = view.toolbar.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(
            &ids[..6],
            ["section:http", "section:websocket", "section:messaging", "section:graphql", "section:webflow", "section:feign"]
        );

        endpoints.handle("endpoints", ViewEvent::Button("section:graphql".into()), &host.invocation()).unwrap();
        assert_eq!(host.refreshed_views(), ["endpoints"]);
        let view = endpoints.render("endpoints", None, &host.invocation()).unwrap();
        assert!(matches!(view.body, Body::Tree(_)));
        assert!(view.toolbar[3].label.starts_with('\u{25cf}'));
        assert_eq!(host.storage()[SECTION_KEY], "graphql");

        endpoints.handle("endpoints", ViewEvent::Button("section:webflow".into()), &host.invocation()).unwrap();
        let view = endpoints.render("endpoints", None, &host.invocation()).unwrap();
        assert!(matches!(view.body, Body::Message(_)));
    }

    #[test]
    fn refresh_in_a_section_refreshes_the_endpoints_window() {
        let host = setup(GRAPHQL_POM);
        let mut endpoints = Endpoints::default();
        for section in ["websocket", "messaging", "graphql", "feign", "http"] {
            endpoints.handle("endpoints", ViewEvent::Button(format!("section:{section}")), &host.invocation()).unwrap();
            let before = host.refreshed_views().len();
            endpoints.handle("endpoints", ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
            assert_eq!(host.refreshed_views()[before..], ["endpoints"], "{section}");
        }
    }

    #[test]
    fn ids_are_unique_across_modules() {
        let manifest = include_str!("../plugin.toml");
        let ids: Vec<&str> =
            manifest.lines().filter_map(|line| line.strip_prefix("id = \"")).filter_map(|rest| rest.strip_suffix('"')).collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "{ids:?}");

        let host = setup(WEB_POM);
        let mut endpoints = Endpoints::default();
        assert!(endpoints.run_command("nowhere.run", &host.invocation(), None).is_err());

        let keys: Vec<&str> = include_str!("../settings.schema.json")
            .lines()
            .filter_map(|line| line.trim().strip_suffix("\": {"))
            .map(|key| key.trim_start_matches('"'))
            .filter(|key| key.starts_with("spring"))
            .collect();
        let mut unique = keys.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), keys.len());
        assert!(keys.len() >= 12, "{keys:?}");
    }

    #[test]
    fn endpoints_item_menu_still_targets_the_endpoints_view() {
        let manifest = include_str!("../plugin.toml");
        assert!(manifest.contains("[[contributes.views]]\nid = \"endpoints\""));
        let actions = include_str!("ui_actions.json");
        for command in ["copy-endpoint-curl", "create-endpoint-http"] {
            assert!(actions.contains(command), "{command}");
            assert!(actions.contains("viewItemKind == route"));
        }

        let host = setup(WEB_POM);
        let mut endpoints = Endpoints::default();
        let view = endpoints.render("endpoints", None, &host.invocation()).unwrap();
        let Body::List(rows) = view.body else { panic!("route list expected") };
        assert!(rows.iter().all(|row| row.key.starts_with("route")), "{:?}", rows.iter().map(|r| &r.key).collect::<Vec<_>>());
    }

    #[test]
    fn merged_providers_stay_under_budget_on_a_sample_project() {
        let host = MockHost::install();
        host.add_file("pom.xml", GRAPHQL_POM);
        for n in 0..40 {
            host.add_file(
                &format!("src/main/java/com/acme/m{n}/Service{n}.java"),
                &format!("package com.acme.m{n};\n\npublic class Service{n} {{}}\n"),
            );
        }
        host.add_file("src/main/resources/graphql/schema.graphqls", SCHEMA);
        let mut endpoints = Endpoints::default();
        let java = doc(JAVA_PATH, "java", JAVA);
        inspect(&host, &mut endpoints, &java);

        let started = Instant::now();
        endpoints.completion(&java, Position::new(4, 20), None);
        endpoints.definition(&java, Position::new(4, 20));
        endpoints.code_actions(&java, Range::default());
        inspect(&host, &mut endpoints, &java);
        assert!(started.elapsed() < Duration::from_millis(10), "{:?}", started.elapsed());
    }
}

impl openidea_plugin::endpoints::EndpointsProvider for Endpoints {
    fn endpoints(&mut self, ctx: &Invocation) -> Result<Vec<openidea_plugin::endpoints::Endpoint>, Error> {
        let Some(project) = ctx.project() else {
            return Ok(Vec::new());
        };
        let found = self.web.ide_routes(&project, spring_text::index::VIEW_BUDGET);
        Ok(ui_kit::routes::endpoints(&found.routes, "Spring"))
    }
}

impl openidea_plugin::file_actions::FileActions for Endpoints {
    fn file_actions(&mut self,doc:&Document,selection:Range)->Vec<openidea_plugin::file_actions::FileAction>{
        let mut actions=self.path_actions.list(doc,selection);
        actions.extend(openidea_plugin::file_actions::FileActions::file_actions(&mut self.webflow,doc,selection));actions
    }
    fn resolve_file_action(&mut self,doc:&Document,action:openidea_plugin::file_actions::FileAction)->Result<openidea_plugin::file_actions::FileAction,Error>{
        if action.data.as_deref().is_some_and(|key|key.starts_with("spring-path-variable:")){return self.path_actions.resolve(doc,action);}
        openidea_plugin::file_actions::FileActions::resolve_file_action(&mut self.webflow,doc,action)
    }
}

#[cfg(test)]
mod transition_owner_tests {
    use super::*;
    use openidea_plugin::{file_actions::FileActions,mock::MockHost,text::position};
    #[test]
    fn merged_owner_routes_xml_fileactions_without_duplicate_source_writes(){
        let h=MockHost::install();let text="<flow xmlns='http://www.springframework.org/schema/webflow'><view-state id='edit'/><end-state id='done'/></flow>";
        h.add_file("flow.xml",text);let d=Document{path:"/project/flow.xml".into(),language:"xml".into(),version:4,text:text.into()};
        let p=position(text,text.find("view-state").unwrap());let mut owner=Endpoints::default();let a=owner.file_actions(&d,Range::new(p,p)).remove(0);
        for answer in ["done","next","yes"] {h.answer_next_prompt(Some(&[answer]));}
        let resolved=owner.resolve_file_action(&d,a).unwrap();assert_eq!(resolved.edits.len(),1);assert!(resolved.operations.is_empty());
        assert_eq!(h.invocation().project().unwrap().read_file("flow.xml").unwrap(),text);assert!(h.process_runs().is_empty());
        h.open_editor(&d.path,"xml",text,&[]);
        owner.handle("endpoints",ViewEvent::Button("section:webflow".into()),&h.invocation()).unwrap();
        let view=owner.render("endpoints",None,&h.invocation()).unwrap();
        let openidea_plugin::views::Body::Html(html)=view.body else{panic!("packaged graph")};
        assert!(html.contains("webflow-graph.css"));assert!(!html.contains("<script>"));assert!(!html.contains("<style>"));
    }
}

impl openidea_plugin::language_features::LanguageFeatures for Endpoints {
    fn code_lenses(&mut self,doc:&Document)->Vec<openidea_plugin::language_features::CodeLens>{
        if !matches!(doc.language.as_str(),"java"|"kotlin"){return Vec::new()}
        let mut lenses=if self.wants_web(doc){crate::source_lenses::lenses(doc)}else{Vec::new()};
        if self.wants_cloud(doc){lenses.extend(crate::source_lenses::feign_lenses(doc));}
        if self.wants_graphql(doc){
            let operations=self.graphql.source_operations(doc);
            lenses.extend(crate::source_lenses::graphql_lenses(doc).into_iter().filter(|l|operations.iter().any(|(range,op)|*range==l.range&&matches!(op.as_str(),"query"|"mutation"))));
            lenses.extend(crate::subscription_drafts::lenses(doc).into_iter().filter(|l|operations.iter().any(|(range,op)|*range==l.range&&op=="subscription")));
        }lenses
    }
}

impl openidea_plugin::language_features::Injections for Endpoints {
    fn injections(&mut self,doc:&Document)->Vec<openidea_plugin::language_features::Injection>{
        if doc.language=="html" && host::setting_bool("springWeb.thymeleafSpelInjection").unwrap_or(false) && self.wants_web(doc) {
            crate::template_links::injections(&doc.text)
        } else if doc.language=="java" && host::setting_bool("springWeb.thymeleafSpelInjection").unwrap_or(false) && self.wants_web(doc) {
            crate::template_links::java_default_value_injections(&doc.path,&doc.text)
        } else { Vec::new() }
    }
}
