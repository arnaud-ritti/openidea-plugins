use crate::{OWNER_WINDOW, WINDOW_SECTION, WebTools};
use cloud::CloudTools;
use graphql::GraphQlTools;
use messaging::Messaging;
use openidea_plugin::providers::{CodeAction, CompletionItem, Document, LineMarker, Providers};
use openidea_plugin::views::{View, ViewEvent, Viewport, Views};
use openidea_plugin::{Diagnostic, Error, Event, Invocation, Location, Plugin, Position, Range, host};
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

const WEB_LANGUAGES: [&str; 3] = ["java", "kotlin", "xml"];
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

pub struct Endpoints {
    web: WebTools,
    websocket: WebSocketTools,
    messaging: Messaging,
    graphql: GraphQlTools,
    webflow: WebFlow,
    cloud: CloudTools,
    sections: Switcher,
    applies: BTreeMap<String, (Instant, Applies)>,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            web: WebTools::default(),
            websocket: WebSocketTools::default(),
            messaging: Messaging::default(),
            graphql: GraphQlTools::default(),
            webflow: WebFlow::default(),
            cloud: CloudTools::default(),
            sections: Switcher::new(SECTION_KEY, SECTIONS),
            applies: BTreeMap::new(),
        }
    }
}

impl Endpoints {
    fn applicable(&mut self, doc: &Document) -> Applies {
        let Some(project) = project_of(&doc.path) else { return Applies::default() };
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
        let Some(data) = action.data.as_deref() else { return Ok(action) };
        let Some((module, rest)) = data.split_once(':') else { return Err(Error::Invalid(format!("code action {data}"))) };
        let routed = CodeAction { data: Some(rest.to_owned()), ..action };
        match module {
            WINDOW_SECTION => self.web.resolve_code_action(doc, routed).map(|resolved| module_data(module, resolved)),
            _ => Err(Error::Invalid(format!("code action {data}"))),
        }
    }

    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let mut markers = Vec::new();
        let mut problems: Vec<Diagnostic> = Vec::new();
        let mut ran = false;
        if self.wants_web(doc) {
            markers.extend(self.web.line_markers(doc));
            problems.extend(crate::take_problems());
            ran = true;
        }
        if self.wants_websocket(doc) {
            markers.extend(self.websocket.line_markers(doc));
            problems.extend(websocket::take_problems());
            ran = true;
        }
        if self.wants_messaging(doc) {
            markers.extend(self.messaging.line_markers(doc));
            problems.extend(messaging::take_problems());
            ran = true;
        }
        if self.wants_graphql(doc) {
            markers.extend(self.graphql.line_markers(doc));
            problems.extend(graphql::take_problems());
            ran = true;
        }
        if self.wants_webflow(doc) {
            markers.extend(self.webflow.line_markers(doc));
            problems.extend(webflow::take_problems());
            ran = true;
        }
        if self.wants_cloud(doc) {
            markers.extend(self.cloud.line_markers(doc));
            problems.extend(cloud::take_problems());
            ran = true;
        }
        if ran {
            let _ = host::publish_diagnostics(&doc.path, &problems);
        }
        markers
    }
}

fn module_view(module: &str, name: &str) -> String {
    format!("{module}.{name}")
}

impl Views for Endpoints {
    fn render(&mut self, _view: &str, viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        let inner = match self.sections.active() {
            "websocket" => self.websocket.render(&module_view(websocket::WINDOW_SECTION, "destinations"), viewport, ctx)?,
            "messaging" => self.messaging.render(&module_view(messaging::WINDOW_SECTION, "messaging"), viewport, ctx)?,
            "graphql" => self.graphql.render(&module_view(graphql::WINDOW_SECTION, "endpoints"), viewport, ctx)?,
            "webflow" => self.webflow.render(&module_view(webflow::WINDOW_SECTION, "flow"), viewport, ctx)?,
            "feign" => self.cloud.render(&module_view(cloud::WINDOW_SECTION, "feign-clients"), viewport, ctx)?,
            _ => self.web.render(&module_view(WINDOW_SECTION, "endpoints"), viewport, ctx)?,
        };
        Ok(self.sections.buttons(inner, &[]))
    }

    fn handle(&mut self, _view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
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
    fn run_command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        self.web.run_command(id, ctx, arg)
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
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

    const JAVA_PATH: &str = "/project/src/main/java/com/acme/C.java";
    const JAVA: &str = "package com.acme;\n\n@Controller\nclass C {\n    @GetMapping(\"/u/{id}\")\n    String get(@PathVariable(\"uid\") Long id) { return \"\"; }\n\n    @QueryMapping\n    String nope() { return \"\"; }\n}\n";
    const SCHEMA: &str = "type Query {\n  books: [String]\n}\n";
    const WEB_POM: &str = "<project><artifactId>spring-boot-starter-web</artifactId></project>";
    const GRAPHQL_POM: &str = "<project><artifactId>spring-boot-starter-web</artifactId><artifactId>spring-boot-starter-graphql</artifactId></project>";
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
        host.diagnostics().get(JAVA_PATH).into_iter().flatten().filter_map(|d| d.code.clone()).collect()
    }

    #[test]
    fn providers_skip_modules_that_do_not_apply() {
        let host = setup(NO_SPRING_POM);
        let mut endpoints = Endpoints::default();
        endpoints.line_markers(&doc(JAVA_PATH, "java", JAVA));
        assert!(host.diagnostics().is_empty());

        let host = setup(GRAPHQL_POM);
        let mut endpoints = Endpoints::default();
        endpoints.line_markers(&doc(JAVA_PATH, "java", JAVA));
        assert!(!host.diagnostics().is_empty());
    }

    #[test]
    fn a_project_with_no_spring_dependency_does_no_module_work() {
        let host = MockHost::install();
        host.add_file("composer.json", "{}");
        host.add_file("src/Thing.java", "class Thing {}");
        let mut endpoints = Endpoints::default();
        let thing = doc("/project/src/Thing.java", "java", "class Thing {}");
        assert!(endpoints.line_markers(&thing).is_empty());
        assert!(endpoints.completion(&thing, Position::new(0, 0), None).is_empty());
        assert!(endpoints.definition(&thing, Position::new(0, 0)).is_empty());
        assert!(endpoints.code_actions(&thing, Range::default()).is_empty());
        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn one_failing_module_does_not_hide_the_others() {
        let host = setup(GRAPHQL_POM);
        host.add_file("src/main/resources/graphql/schema.graphqls", "{ not sdl");
        let mut endpoints = Endpoints::default();
        endpoints.line_markers(&doc(JAVA_PATH, "java", JAVA));
        assert!(codes(&host).contains(&"unknown-path-variable".to_owned()), "{:?}", codes(&host));
    }

    #[test]
    fn diagnostics_of_every_module_are_published_together() {
        let host = setup(GRAPHQL_POM);
        let mut endpoints = Endpoints::default();
        endpoints.line_markers(&doc(JAVA_PATH, "java", JAVA));
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
        assert_eq!(&ids[..6], ["section:http", "section:websocket", "section:messaging", "section:graphql", "section:webflow", "section:feign"]);

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
        let ids: Vec<&str> = manifest.lines().filter_map(|line| line.strip_prefix("id = \"")).filter_map(|rest| rest.strip_suffix('"')).collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "{ids:?}");

        let host = setup(WEB_POM);
        let mut endpoints = Endpoints::default();
        assert!(endpoints.run_command("nowhere.run", &host.invocation(), None).is_err());

        let keys: Vec<&str> = include_str!("../settings.schema.json").lines().filter_map(|line| line.trim().strip_suffix("\": {")).map(|key| key.trim_start_matches('"')).filter(|key| key.starts_with("spring")).collect();
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
        for command in ["copy-endpoint-curl", "create-endpoint-http"] {
            let menu = format!("menu = \"viewItem.context\"\nview = \"endpoints\"\ncommand = \"{command}\"\nwhen = \"viewItemKind == route\"");
            assert!(manifest.contains(&menu), "{command}");
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
            host.add_file(&format!("src/main/java/com/acme/m{n}/Service{n}.java"), &format!("package com.acme.m{n};\n\npublic class Service{n} {{}}\n"));
        }
        host.add_file("src/main/resources/graphql/schema.graphqls", SCHEMA);
        let mut endpoints = Endpoints::default();
        let java = doc(JAVA_PATH, "java", JAVA);
        endpoints.line_markers(&java);

        let started = Instant::now();
        endpoints.completion(&java, Position::new(4, 20), None);
        endpoints.definition(&java, Position::new(4, 20));
        endpoints.code_actions(&java, Range::default());
        endpoints.line_markers(&java);
        assert!(started.elapsed() < Duration::from_millis(10), "{:?}", started.elapsed());
    }
}
