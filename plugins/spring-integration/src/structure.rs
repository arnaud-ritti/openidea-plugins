use crate::{Integration, OWNER_WINDOW};
use batch::Batch;
use modulith::ModulithTools;
use openidea_plugin::providers::{CodeAction, CompletionItem, Document, LineMarker, Providers};
use openidea_plugin::views::{View, ViewEvent, Viewport, Views};
use openidea_plugin::{Diagnostic, Error, Invocation, Location, Plugin, Position, Range, host};
use spring_text::index::project_of;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use ui_kit::sections::{SectionDef, Switcher};

const SECTIONS: &[SectionDef] = &[
    SectionDef { id: "channels", label: "Channels" },
    SectionDef { id: "batch", label: "Batch" },
    SectionDef { id: "modules", label: "Modules" },
    SectionDef { id: "diagram", label: "Diagram" },
];
const SECTION_KEY: &str = "structure.section";
const APPLIES_TTL: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Default)]
struct Applies {
    integration: bool,
    batch: bool,
    modulith: bool,
}

pub struct Structure {
    integration: Integration,
    batch: Batch,
    modulith: ModulithTools,
    sections: Switcher,
    applies: BTreeMap<String, (Instant, Applies)>,
}

impl Default for Structure {
    fn default() -> Self {
        Self {
            integration: Integration::default(),
            batch: Batch::default(),
            modulith: ModulithTools::default(),
            sections: Switcher::new(SECTION_KEY, SECTIONS),
            applies: BTreeMap::new(),
        }
    }
}

impl Structure {
    fn applicable(&mut self, doc: &Document) -> Applies {
        let Some(project) = project_of(&doc.path) else { return Applies::default() };
        let root = project.root().unwrap_or_default();
        if let Some((at, applies)) = self.applies.get(&root)
            && at.elapsed() < APPLIES_TTL
        {
            return *applies;
        }

        let applies = Applies {
            integration: crate::applies(&project),
            batch: batch::applies(&project),
            modulith: modulith::applies(&project),
        };
        self.applies.insert(root, (Instant::now(), applies));
        applies
    }

    fn wants_integration(&mut self, doc: &Document) -> bool {
        matches!(doc.language.as_str(), "java" | "xml") && self.applicable(doc).integration
    }

    fn wants_batch(&mut self, doc: &Document) -> bool {
        self.applicable(doc).batch
    }

    fn wants_modulith(&mut self, doc: &Document) -> bool {
        doc.language == "java" && self.applicable(doc).modulith
    }
}

fn module_data(module: &str, action: CodeAction) -> CodeAction {
    CodeAction { data: action.data.map(|data| format!("{module}:{data}")), ..action }
}

impl Providers for Structure {
    fn completion(&mut self, doc: &Document, at: Position, trigger: Option<&str>) -> Vec<CompletionItem> {
        let mut items = Vec::new();
        if self.wants_integration(doc) {
            items.extend(self.integration.completion(doc, at, trigger));
        }
        if self.wants_batch(doc) {
            items.extend(self.batch.completion(doc, at, trigger));
        }
        if self.wants_modulith(doc) {
            items.extend(self.modulith.completion(doc, at, trigger));
        }
        items
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let mut found = Vec::new();
        if self.wants_integration(doc) {
            found.extend(self.integration.definition(doc, at));
        }
        if self.wants_batch(doc) {
            found.extend(self.batch.definition(doc, at));
        }
        found
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        if !self.wants_modulith(doc) {
            return Vec::new();
        }
        let actions = self.modulith.code_actions(doc, selection);
        actions.into_iter().map(|action| module_data(modulith::WINDOW_SECTION, action)).collect()
    }

    fn resolve_code_action(&mut self, doc: &Document, action: CodeAction) -> Result<CodeAction, Error> {
        let Some(data) = action.data.as_deref() else { return Ok(action) };
        let Some((module, rest)) = data.split_once(':') else { return Err(Error::Invalid(format!("code action {data}"))) };
        let routed = CodeAction { data: Some(rest.to_owned()), ..action };
        match module {
            modulith::WINDOW_SECTION => self.modulith.resolve_code_action(doc, routed).map(|resolved| module_data(module, resolved)),
            batch::WINDOW_SECTION => self.batch.resolve_code_action(doc, routed).map(|resolved| module_data(module, resolved)),
            crate::WINDOW_SECTION => self.integration.resolve_code_action(doc, routed).map(|resolved| module_data(module, resolved)),
            _ => Err(Error::Invalid(format!("code action {data}"))),
        }
    }

    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let mut markers = Vec::new();
        let mut problems: Vec<Diagnostic> = Vec::new();
        let mut ran = false;
        if self.wants_integration(doc) {
            markers.extend(self.integration.line_markers(doc));
            problems.extend(crate::take_problems());
            ran = true;
        }
        if self.wants_batch(doc) {
            markers.extend(self.batch.line_markers(doc));
            problems.extend(batch::take_problems());
            ran = true;
        }
        if self.wants_modulith(doc) {
            markers.extend(self.modulith.line_markers(doc));
            problems.extend(modulith::take_problems());
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

impl Views for Structure {
    fn render(&mut self, _view: &str, viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        let active = self.sections.active();
        let inner = match active {
            "batch" => self.batch.render(&module_view(batch::WINDOW_SECTION, "diagram"), viewport, ctx)?,
            "modules" | "diagram" => self.modulith.render(&module_view(modulith::WINDOW_SECTION, active), viewport, ctx)?,
            _ => self.integration.render(&module_view(crate::WINDOW_SECTION, "diagram"), viewport, ctx)?,
        };
        Ok(self.sections.buttons(inner, &[]))
    }

    fn handle(&mut self, _view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if self.sections.handle(&event) {
            host::refresh_view(OWNER_WINDOW);
            return Ok(());
        }

        let active = self.sections.active();
        match active {
            "batch" => self.batch.handle(&module_view(batch::WINDOW_SECTION, "diagram"), event, ctx),
            "modules" | "diagram" => self.modulith.handle(&module_view(modulith::WINDOW_SECTION, active), event, ctx),
            _ => self.integration.handle(&module_view(crate::WINDOW_SECTION, "diagram"), event, ctx),
        }
    }
}

impl Plugin for Structure {
    fn run_command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        let module = id.split_once('.').map(|(module, _)| module);
        match module {
            Some(modulith::WINDOW_SECTION) => self.modulith.run_command(id, ctx, arg),
            Some(batch::WINDOW_SECTION) => self.batch.run_command(id, ctx, arg),
            Some(crate::WINDOW_SECTION) => self.integration.run_command(id, ctx, arg),
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::text::position;
    use openidea_plugin::views::Body;

    const XML_PATH: &str = "/project/src/main/resources/integration.xml";
    const SHIPPER_PATH: &str = "/project/src/main/java/com/acme/shipping/Shipper.java";
    const XML: &str = "<beans xmlns=\"http://www.springframework.org/schema/beans\" xmlns:int=\"http://www.springframework.org/schema/integration\">
<int:channel id=\"orders\"/>
<int:service-activator input-channel=\"orders\" output-channel=\"shipping\"/>
<int:service-activator input-channel=\"shipping\" output-channel=\"orders\"/>
</beans>";
    const SHIPPER: &str = "package com.acme.shipping;\n\nimport com.acme.order.internal.OrderRepository;\n\nclass Shipper {}\n";
    const INTEGRATION_POM: &str = "<project><artifactId>spring-boot-starter-integration</artifactId></project>";
    const WEB_POM: &str = "<project><artifactId>spring-boot-starter-web</artifactId></project>";

    fn setup(pom: &str) -> MockHost {
        let host = MockHost::install();
        host.add_file("pom.xml", pom);
        host.add_file("src/main/resources/integration.xml", XML);
        host.add_file("src/main/java/com/acme/Application.java", "package com.acme;\n\n@SpringBootApplication\npublic class Application {}\n");
        host.add_file("src/main/java/com/acme/order/OrderService.java", "package com.acme.order;\n\npublic class OrderService {}\n");
        host.add_file("src/main/java/com/acme/order/internal/OrderRepository.java", "package com.acme.order.internal;\n\nclass OrderRepository {}\n");
        host.add_file("src/main/java/com/acme/order/package-info.java", "@ApplicationModule\npackage com.acme.order;\n");
        host.add_file("src/main/java/com/acme/shipping/package-info.java", "@ApplicationModule\npackage com.acme.shipping;\n");
        host.add_file("src/main/java/com/acme/shipping/Shipper.java", SHIPPER);
        host
    }

    fn doc(path: &str, language: &str, text: &str) -> Document {
        Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
    }

    #[test]
    fn providers_skip_modules_that_do_not_apply() {
        let _host = setup(WEB_POM);
        let mut structure = Structure::default();
        assert!(structure.line_markers(&doc(XML_PATH, "xml", XML)).is_empty());
        let project = spring_text::index::project_of(XML_PATH).unwrap();
        assert!(structure.integration.indexes.get(&project, Duration::ZERO).files().is_empty());

        let _host = setup(INTEGRATION_POM);
        let mut structure = Structure::default();
        assert!(!structure.line_markers(&doc(XML_PATH, "xml", XML)).is_empty());
    }

    #[test]
    fn a_project_with_no_spring_dependency_does_no_module_work() {
        let host = MockHost::install();
        host.add_file("composer.json", "{}");
        host.add_file("src/Thing.java", "class Thing {}");
        let mut structure = Structure::default();
        let thing = doc("/project/src/Thing.java", "java", "class Thing {}");
        assert!(structure.line_markers(&thing).is_empty());
        assert!(structure.completion(&thing, Position::new(0, 0), None).is_empty());
        assert!(structure.definition(&thing, Position::new(0, 0)).is_empty());
        assert!(structure.code_actions(&thing, Range::default()).is_empty());
        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn one_failing_module_does_not_hide_the_others() {
        let _host = setup("<project><artifactId>spring-integration</artifactId><artifactId>spring-batch</artifactId></project>");
        let mut structure = Structure::default();
        let at = position(XML, XML.find("input-channel=\"orders\"").unwrap() + 20);
        assert!(!structure.definition(&doc(XML_PATH, "xml", XML), at).is_empty());
        assert!(structure.batch.definition(&doc(XML_PATH, "xml", XML), at).is_empty());
    }

    #[test]
    fn diagnostics_of_every_module_are_published_together() {
        let host = setup("<project><module>app</module></project>");
        let mut structure = Structure::default();
        structure.line_markers(&doc(SHIPPER_PATH, "java", SHIPPER));
        let codes: Vec<_> = host.diagnostics()[SHIPPER_PATH].iter().filter_map(|d| d.code.clone()).collect();
        assert_eq!(codes, ["module-internal"]);

        structure.line_markers(&doc(XML_PATH, "xml", XML));
        assert!(host.diagnostics().contains_key(SHIPPER_PATH));
    }

    #[test]
    fn code_action_resolves_through_its_module() {
        let _host = setup("<project><artifactId>spring-modulith-core</artifactId></project>");
        let mut structure = Structure::default();
        let shipper = doc(SHIPPER_PATH, "java", SHIPPER);
        let at = position(SHIPPER, SHIPPER.find("order.internal").unwrap());
        let actions = structure.code_actions(&shipper, Range::new(at, at));
        assert_eq!(actions.len(), 1);

        let routed = CodeAction { data: Some("modulith:ref".into()), ..actions[0].clone() };
        let resolved = structure.resolve_code_action(&shipper, routed).unwrap();
        assert_eq!(resolved.data.as_deref(), Some("modulith:ref"));
        assert_eq!(resolved.edits, actions[0].edits);

        let unknown = CodeAction { data: Some("nowhere:ref".into()), ..actions[0].clone() };
        assert!(structure.resolve_code_action(&shipper, unknown).is_err());
    }

    #[test]
    fn section_buttons_switch_the_structure_window() {
        let host = setup(INTEGRATION_POM);
        let mut structure = Structure::default();
        let view = structure.render("structure", None, &host.invocation()).unwrap();
        assert!(matches!(view.body, Body::Html(_)));
        let ids: Vec<_> = view.toolbar.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(&ids[..4], ["section:channels", "section:batch", "section:modules", "section:diagram"]);

        structure.handle("structure", ViewEvent::Button("section:modules".into()), &host.invocation()).unwrap();
        assert_eq!(host.refreshed_views(), ["structure"]);
        let view = structure.render("structure", None, &host.invocation()).unwrap();
        assert!(matches!(view.body, Body::Tree(_) | Body::Message(_)));
        assert!(view.toolbar[2].label.starts_with('\u{25cf}'));
        assert_eq!(host.storage()[SECTION_KEY], "modules");

        structure.handle("structure", ViewEvent::Button("section:diagram".into()), &host.invocation()).unwrap();
        let view = structure.render("structure", None, &host.invocation()).unwrap();
        assert!(matches!(view.body, Body::Html(_)));
    }

    #[test]
    fn refresh_in_a_section_refreshes_the_structure_window() {
        let host = setup(INTEGRATION_POM);
        let mut structure = Structure::default();
        structure.handle("structure", ViewEvent::Button("section:batch".into()), &host.invocation()).unwrap();
        let before = host.refreshed_views().len();
        structure.handle("structure", ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
        assert_eq!(host.refreshed_views()[before..], ["structure"]);
    }

    #[test]
    fn ids_are_unique_across_modules() {
        let manifest = include_str!("../plugin.toml");
        let ids: Vec<&str> = manifest.lines().filter_map(|line| line.strip_prefix("id = \"")).filter_map(|rest| rest.strip_suffix('"')).collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "{ids:?}");
        assert!(ids.contains(&"modulith.copy-docs"));

        let host = setup(INTEGRATION_POM);
        let mut structure = Structure::default();
        structure.run_command("modulith.copy-docs", &host.invocation(), None).unwrap();
        assert!(structure.run_command("copy-docs", &host.invocation(), None).is_err());
        assert!(structure.run_command("batch.copy-docs", &host.invocation(), None).is_err());
    }

    #[test]
    fn merged_providers_stay_under_budget_on_a_sample_project() {
        let host = MockHost::install();
        host.add_file("pom.xml", "<project><module>app</module></project>");
        for n in 0..40 {
            host.add_file(&format!("src/main/java/com/acme/m{n}/Service{n}.java"), &format!("package com.acme.m{n};\n\npublic class Service{n} {{}}\n"));
            host.add_file(&format!("src/main/resources/flow{n}.xml"), &XML.replace("orders", &format!("orders{n}")));
        }
        host.add_file("src/main/java/com/acme/Application.java", "package com.acme;\n\n@SpringBootApplication\npublic class Application {}\n");
        let mut structure = Structure::default();
        let document = doc(XML_PATH, "xml", XML);
        let java = doc("/project/src/main/java/com/acme/m1/Service1.java", "java", "package com.acme.m1;\n\npublic class Service1 {}\n");
        structure.line_markers(&document);

        let started = Instant::now();
        structure.completion(&document, Position::new(2, 40), None);
        structure.definition(&document, Position::new(2, 40));
        structure.code_actions(&java, Range::default());
        structure.line_markers(&document);
        assert!(started.elapsed() < Duration::from_millis(10), "{:?}", started.elapsed());
    }
}
