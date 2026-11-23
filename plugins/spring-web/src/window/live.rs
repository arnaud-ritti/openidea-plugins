//! The registered wrapper owns one merged, current-text diagnostic response.
use super::*;
use crate::*;
use openidea_plugin::diagnostics::DiagnosticsProvider;
use openidea_plugin::hover::{Hover, HoverProvider};
use openidea_plugin::{Diagnostic, EventKind};
const MAX_TEXT: usize = 128 << 10;
const GLOBS: &[&str] = &[
    "**/*.java",
    "**/*.kt",
    "**/*.xml",
    "**/*.graphql",
    "**/*.graphqls",
    "**/*.properties",
    "**/*.yaml",
    "**/*.yml",
    "**/pom.xml",
    "**/*.gradle",
    "**/*.gradle.kts",
    "**/*.toml",
];
const TEMPLATE_GLOBS:&[&str]=&["**/*.html","**/*.jsp","**/*.ftlh","**/*.ftl","**/*.mustache","**/*.peb","**/*.jte","**/*.kte"];
fn bounded(path: &str, text: &str) -> Option<bool> {
    Some(
        text.len() <= MAX_TEXT
            && (![".java", ".kt", ".graphql", ".graphqls"].iter().any(|extension| path.ends_with(extension)) || spring_text::text::balanced(text, path.ends_with(".kt"))),
    )
}
const WEB: usize = 0;
const WEBSOCKET: usize = 1;
const MESSAGING: usize = 2;
const GRAPHQL: usize = 3;
const WEBFLOW: usize = 4;
const CLOUD: usize = 5;
/// Per module, the files its absence claims depend on.
const BOUND_GLOBS: [&[&str]; 6] = [
    &["**/*.java", "**/*.kt"],
    &["**/*.java", "**/*.kt", "**/*.xml"],
    &["**/*.java", "**/*.kt", "**/*.xml"],
    &["**/*.java", "**/*.kt", "**/*.graphql", "**/*.graphqls"],
    &["**/*.java", "**/*.xml"],
    &["**/*.java", "**/*.kt", "**/application*.properties", "**/application*.yml", "**/application*.yaml", "**/bootstrap*.properties", "**/bootstrap*.yml", "**/bootstrap*.yaml", "**/pom.xml", "**/*.gradle", "**/*.gradle.kts"],
];
pub(super) struct Live {
    bounds: [Indexes<bool>; 6],
    /// Which XML files import which resources, to tell whether another file may declare a bean.
    imports: Indexes<Vec<String>>,
    watching: bool,
}
impl Default for Live {
    fn default() -> Self {
        Self {
            bounds: BOUND_GLOBS.map(|globs| Indexes::new(globs, bounded)),
            imports: Indexes::new(&["**/*.xml"], xml_checks::imports),
            watching: false,
        }
    }
}
fn event_paths(events: &[Event]) -> Vec<&str> {
    let opened = events.iter().filter_map(|event| match event {
        Event::FileOpened(path) => Some(path.as_str()),
        _ => None,
    });
    spring_text::index::changed_paths(events).into_iter().map(|(path, _)| path).chain(opened).collect()
}
impl Endpoints {
    fn cached_applies(&self, doc: &Document) -> Applies {
        project_of(&doc.path)
            .and_then(|p| p.root().ok())
            .and_then(|root| self.applies.get(&root))
            .filter(|(at, _)| self.live.watching || at.elapsed() < APPLIES_TTL)
            .map(|(_, a)| *a)
            .unwrap_or_default()
    }
    /// Reads the live caches of `project` for at most `budget`. A provider passes
    /// its `cached` verdict: then nothing is discovered and no build file is read,
    /// only passes already under way advance.
    fn warm_live(&mut self, project: &Project, budget: Duration, cached: Option<Applies>) {
        let deadline = Instant::now() + budget;
        let root = project.root().unwrap_or_default();
        if root.is_empty() {
            return;
        }
        let discover = cached.is_none();
        let applies = cached.unwrap_or_else(|| {
            let doc = Document { path: format!("{root}/__live.java"), language: "java".into(), version: 0, text: String::new() };
            self.applicable(&doc)
        });
        let wanted = [applies.web, applies.websocket, applies.messaging, applies.graphql, applies.webflow, applies.cloud];
        if !wanted.contains(&true) {
            return;
        }
        let part = || deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(120));
        for (module, bounds) in self.live.bounds.iter_mut().enumerate() {
            if part().is_zero() {
                return;
            }
            if wanted[module] {
                bounds.warm(project, part(), discover);
            }
        }
        if applies.web && !part().is_zero() {
            self.web.live_warm(project, part(), discover);
            self.live.imports.warm(project, part(), discover);
        }
        if applies.websocket && !part().is_zero() {
            self.websocket.live_warm(project, part(), discover);
        }
        if applies.messaging && !part().is_zero() {
            self.messaging.live_warm(project, part(), discover);
        }
        if applies.graphql && !part().is_zero() {
            self.graphql.live_warm(project, part(), discover);
        }
        if applies.webflow && !part().is_zero() {
            self.webflow.live_warm(project, part(), discover);
        }
        if applies.cloud && !part().is_zero() {
            self.cloud.live_warm(project, part(), discover);
        }
    }
    pub(crate) fn activate_live(&mut self) {
        let globs=GLOBS.iter().chain(TEMPLATE_GLOBS).copied().collect::<Vec<_>>();
        self.live.watching = host::subscribe_paths("files-changed", &globs).is_ok();
        let _ = host::subscribe(EventKind::FileOpened);
        let _ = host::subscribe(EventKind::FileSaved);
        let deadline = Instant::now() + spring_text::index::COMMAND_BUDGET;
        for project in host::projects().unwrap_or_default() {
            let budget = deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(600));
            if budget.is_zero() {
                break;
            }
            self.warm_live(&project, budget, None);
        }
    }
    pub(super) fn live_events(&mut self, events: &[Event]) {
        let deadline = Instant::now() + spring_text::index::COMMAND_BUDGET;
        let mut touched = BTreeMap::new();
        for path in event_paths(events) {
            let template = TEMPLATE_GLOBS.iter().any(|glob| spring_text::index::glob_matches(glob, path));
            if !template && !GLOBS.iter().any(|g| spring_text::index::glob_matches(g, path)) {
                continue;
            }
            let Some(project) = project_of(path) else {
                continue;
            };
            let Ok(root) = project.root() else {
                continue;
            };
            if template {
                let prefix = format!("{root}\n");
                self.web.listings.retain(|key, _| !key.starts_with(&prefix));
                continue;
            }
            if path.ends_with("pom.xml") || path.ends_with(".gradle") || path.ends_with(".gradle.kts") {
                self.applies.remove(&root);
            }
            touched.insert(root, project);
        }
        for bounds in &mut self.live.bounds {
            bounds.on_events(events, deadline);
        }
        self.live.imports.on_events(events, deadline);
        self.web.versioning.on_events(events, deadline);
        self.web.models.on_events(events, deadline);
        self.web.endpoints.on_events(events, deadline);
        self.web.settings.on_events(events, deadline);
        self.websocket.live_events(events, deadline);
        self.messaging.live_events(events, deadline);
        self.graphql.live_events(events, deadline);
        self.webflow.live_events(events, deadline);
        self.cloud.live_events(events, deadline);
        for project in touched.values() {
            self.warm_live(project, deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(600)), None);
        }
    }
    fn allow_absence(&mut self, doc: &Document, module: usize) -> bool {
        self.live.watching
            && self.live.bounds[module]
                .for_cached_document(&doc.path, &doc.text)
                .is_some_and(|i| i.fresh() && i.iter().all(|(_, ok)| *ok))
    }
    /// Why a section's absence checks are off, for its view status.
    pub(super) fn absence_note(&mut self, project: &Project, section: &str) -> Option<String> {
        let module = match section {
            "websocket" => WEBSOCKET,
            "messaging" => MESSAGING,
            "graphql" => GRAPHQL,
            "webflow" => WEBFLOW,
            "feign" => CLOUD,
            _ => WEB,
        };
        let index = self.live.bounds[module].get(project, Duration::ZERO);
        if !index.walked() {
            return None;
        }
        if let Some((path, _)) = index.iter().find(|(_, ok)| !**ok) {
            let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
            return Some(format!("absence checks off: {file}"));
        }
        match index.unreadable() {
            0 => None,
            1 => Some("absence checks off: 1 unreadable file".to_owned()),
            n => Some(format!("absence checks off: {n} unreadable files")),
        }
    }
}
impl DiagnosticsProvider for Endpoints {
    fn diagnostics(&mut self, doc: &Document) -> Vec<Diagnostic> {
        if doc.text.len() > MAX_TEXT || matches!(doc.language.as_str(), "java" | "kotlin" | "graphql") && !spring_text::text::balanced(&doc.text, doc.language == "kotlin") {
            return Vec::new();
        }
        let applicable = self.cached_applies(doc);
        if !(applicable.web || applicable.websocket || applicable.messaging || applicable.graphql || applicable.webflow || applicable.cloud)
        {
            return Vec::new();
        }
        if let Some(project) = project_of(&doc.path) {
            self.warm_live(&project, spring_text::index::DIAGNOSTICS_WARM, Some(applicable));
        }
        if applicable.web {
            let _ = self.web.versioning.for_cached_document(&doc.path, &doc.text);
        }
        let mut out = Vec::new();
        if applicable.web && WEB_LANGUAGES.contains(&doc.language.as_str()) {
            if is_xml(doc) {
                let file = doc.path.rsplit('/').next().unwrap_or(&doc.path).to_owned();
                let imports = self.live.imports.for_cached_document(&doc.path, &doc.text).filter(|i| i.fresh());
                let imported_elsewhere = || {
                    let Some(index) = &imports else {
                        return true;
                    };
                    index.iter().any(|(path, found)| path != doc.path && found.iter().any(|resource| resource.contains(&file)))
                };
                out.extend(xml_checks::check_in_project(doc, &imported_elsewhere).iter().map(|f| f.diagnostic(&doc.text)));
            } else {
                out.extend(problems(doc).into_iter().map(|p| p.diagnostic));
                let web_absence = self.allow_absence(doc, WEB);
                let (configured, standard, inserter_missing) = match self.web.versioning.for_cached_document(&doc.path, &doc.text) {
                    Some(index) if web_absence && index.fresh() => {
                        let files: Vec<_> = index.iter().map(|(_, f)| *f).collect();
                        (
                            checks::project_versioning(true, &files),
                            checks::project_standard_parser(true, &files),
                            Some(!files.iter().any(|f| f.inserter || f.is_config_file && f.configured)),
                        )
                    }
                    _ => (None, None, None),
                };
                out.extend(
                    checks::check_java(&parse_source(&doc.path, &doc.text), configured, standard, checks::Groups::from_settings())
                        .iter()
                        .map(|f| f.diagnostic(&doc.text)),
                );
                if inserter_missing == Some(true) && doc.path.contains("/src/test/") && doc.text.contains("apiVersion(") {
                    out.extend(source_calls(&doc.path,&doc.text).into_iter().filter(|c|c.name=="apiVersion" && !c.is_new).map(|c|Diagnostic::new(range(&doc.text,c.at,c.at+c.name.len()),Severity::Hint,"Cached source declares no ApiVersionInserter or spring.mvc.apiversion.* configuration; this test version may not reach the request.").code("api-version-without-inserter")));
                }
            }
        }
        if applicable.websocket && WEBSOCKET_LANGUAGES.contains(&doc.language.as_str()) {
            out.extend({ let absence = self.allow_absence(doc, WEBSOCKET); self.websocket.live_diagnostics(doc, absence) });
        }
        if applicable.messaging && MESSAGING_LANGUAGES.contains(&doc.language.as_str()) {
            out.extend({ let absence = self.allow_absence(doc, MESSAGING); self.messaging.live_diagnostics(doc, absence) });
        }
        if applicable.graphql && GRAPHQL_LANGUAGES.contains(&doc.language.as_str()) {
            out.extend({ let absence = self.allow_absence(doc, GRAPHQL); self.graphql.live_diagnostics(doc, absence) });
        }
        if applicable.webflow && WEBFLOW_LANGUAGES.contains(&doc.language.as_str()) {
            out.extend({ let absence = self.allow_absence(doc, WEBFLOW); self.webflow.live_diagnostics(doc, absence) });
        }
        if applicable.cloud && CLOUD_LANGUAGES.contains(&doc.language.as_str()) {
            out.extend({ let absence = self.allow_absence(doc, CLOUD); self.cloud.live_diagnostics(doc, absence) });
        }
        out.truncate(500);
        out
    }
}
pub(super) fn imported(file: &JavaFile, name: &str) -> bool {
    let expected = if matches!(name, "Controller" | "RestController") {
        if name == "Controller" {
            "org.springframework.stereotype.Controller".into()
        } else {
            "org.springframework.web.bind.annotation.RestController".into()
        }
    } else {
        format!("org.springframework.web.bind.annotation.{name}")
    };
    !file.types.iter().any(|t| t.name == name)
        && file.imports.iter().any(|i| !i.is_static && i.path == expected)
        && !file.imports.iter().any(|i| !i.is_static && i.path.rsplit('.').next() == Some(name) && i.path != expected)
}
fn path_hover(doc: &Document, at: Position) -> Option<Hover> {
    if !matches!(doc.language.as_str(), "java" | "kotlin") {
        return None;
    }
    let off = offset(&doc.text, at)?;
    let file = parse_source(&doc.path, &doc.text);
    if !imported(&file, "PathVariable") {
        return None;
    }
    for class in file
        .types
        .iter()
        .filter(|t| t.annotations.iter().any(|a| matches!(a.name.as_str(), "Controller" | "RestController") && imported(&file, &a.name)))
    {
        if class.annotation("RequestMapping").is_some_and(|a| !literal_paths(a) || !imported(&file, "RequestMapping")) {
            continue;
        }
        for method in class.methods() {
            let maps: Vec<_> = method.annotations.iter().filter(|a| MAPPINGS.contains(&a.name.as_str())).collect();
            if maps.is_empty() || !maps.iter().all(|a| literal_paths(a) && imported(&file, &a.name)) {
                continue;
            }
            let paths = java::mappings(class, method);
            if paths.len() != 1 {
                continue;
            }
            let path = &paths[0].1;
            for param in &method.params {
                let Some(annotation) = param.annotation("PathVariable") else {
                    continue;
                };
                let name = match annotation.args.iter().find(|a| matches!(a.name.as_str(), "name" | "value")) {
                    None => param.name.clone(),
                    Some(a) if a.strings.len() == 1 && java::tokens(&a.text).len() == 1 => a.strings[0].value.clone(),
                    _ => continue,
                };
                if variables(path).iter().filter(|v| v.0 == name).count() != 1
                    || name.len() > 128
                    || name.contains('`')
                    || name.chars().any(char::is_control)
                    || param.ty.text.len() > 128
                    || param.ty.text.contains('`')
                    || param.ty.text.chars().any(char::is_control)
                {
                    continue;
                }
                let bindings = method
                    .params
                    .iter()
                    .filter_map(|p| {
                        p.annotation("PathVariable").map(|a| {
                            match a.args.iter().find(|arg| matches!(arg.name.as_str(), "name" | "value")) {
                                None => p.name.clone(),
                                Some(arg) if arg.strings.len() == 1 && java::tokens(&arg.text).len() == 1 => arg.strings[0].value.clone(),
                                _ => String::new(),
                            }
                        })
                    })
                    .filter(|bound| bound == &name)
                    .count();
                if bindings != 1 {
                    continue;
                }
                let parameter_hit = param.at <= off && off <= param.at + param.name.len() || annotation.at <= off && off <= annotation.end;
                let literal_hit = maps.iter().flat_map(|a| a.strings(&["value", "path"])).find_map(|s| {
                    variables(&doc.text[s.at..s.end])
                        .into_iter()
                        .find(|(n, from, to)| *n == name && s.at + *from <= off && off <= s.at + *to)
                        .map(|(_, from, to)| range(&doc.text, s.at + from, s.at + to))
                });
                if parameter_hit || literal_hit.is_some() {
                    return Some(Hover::new(format!("Path variable `{name}` binds parameter `{}` in this mapping.\n\nDeclared parameter type: `{}`.\n\nOne literal source route; runtime conversion and handler selection are not inferred.",param.name,param.ty.text)).range(literal_hit.unwrap_or_else(||if param.at<=off && off<=param.at+param.name.len(){range(&doc.text,param.at,param.at+param.name.len())}else{range(&doc.text,annotation.at,annotation.end)})));
                }
            }
        }
    }
    None
}
impl HoverProvider for Endpoints {
    fn hover(&mut self, doc: &Document, at: Position) -> Option<Hover> {
        if doc.text.len() > MAX_TEXT || matches!(doc.language.as_str(), "java" | "kotlin" | "graphql") && !spring_text::text::balanced(&doc.text, doc.language == "kotlin") {
            return None;
        }
        let applies = self.cached_applies(doc);
        if applies.web
            && let Some(hover) = path_hover(doc, at)
        {
            return Some(hover);
        }
        if applies.graphql
            && let Some(hover) = self.graphql.live_hover(doc, at)
        {
            return Some(hover);
        }
        if applies.messaging {
            return self.messaging.live_hover(doc, at);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    const POM: &str = "<project><artifactId>spring-boot-starter-web</artifactId><artifactId>spring-boot-starter-graphql</artifactId><artifactId>spring-jms</artifactId></project>";
    const PATH: &str = "/project/C.java";
    fn doc(text: &str) -> Document {
        Document { path: PATH.into(), language: "java".into(), version: 11, text: text.into() }
    }
    fn setup() -> (MockHost, Endpoints) {
        let host = MockHost::install();
        host.add_file("pom.xml", POM);
        host.add_file("src/main/resources/graphql/schema.graphqls", "type Query { books: [String] }");
        let mut tools = Endpoints::default();
        tools.activate_live();
        (host, tools)
    }
    #[test]
    fn truncated_xml_list_yields_no_unknown_bean() {
        let mvc = Document {
            path: "/project/src/main/resources/mvc.xml".into(),
            language: "xml".into(),
            version: 1,
            text: "<b:beans xmlns:b='http://www.springframework.org/schema/beans' xmlns:m='http://www.springframework.org/schema/mvc'><m:annotation-driven validator='nope'/></b:beans>".into(),
        };
        let unknown = |tools: &mut Endpoints| tools.diagnostics(&mvc).iter().any(|d| d.code.as_deref() == Some("mvc-xml-unknown-bean"));
        let (host, mut tools) = setup();
        host.add_file("src/main/resources/mvc.xml", &mvc.text);
        tools.live_events(&[Event::FileSaved(mvc.path.clone())]);
        assert!(unknown(&mut tools), "a fully read project proves nobody imports the file");

        let host = MockHost::install();
        host.add_file("pom.xml", POM);
        host.add_file("src/main/resources/mvc.xml", &mvc.text);
        for n in 0..10_000 {
            host.add_file(&format!("fixtures/f{n}.xml"), "<a/>");
        }
        let mut tools = Endpoints::default();
        tools.activate_live();
        tools.live_events(&[Event::FileSaved(mvc.path.clone())]);
        assert!(!unknown(&mut tools), "a capped listing may hide an importer");
    }
    #[test]
    fn latin1_properties_does_not_disable_web_absence() {
        let (host, mut tools) = setup();
        let test = Document {
            path: "/project/src/test/java/T.java".into(),
            language: "java".into(),
            version: 1,
            text: "class T { void t() { client.get().uri(\"/x\").apiVersion(\"1\").exchange(); } }".into(),
        };
        host.add_file("src/test/java/T.java", &test.text);
        tools.live_events(&[Event::FileSaved(test.path.clone())]);
        let has_hint = |tools: &mut Endpoints| tools.diagnostics(&test).iter().any(|d| d.code.as_deref() == Some("api-version-without-inserter"));
        assert!(has_hint(&mut tools), "absence checks work before the Latin-1 file");
        host.add_file_bytes("src/main/resources/messages_fr.properties", b"caf\xe9=1");
        tools.live_events(&[Event::FileSaved("/project/src/main/resources/messages_fr.properties".into())]);
        assert!(has_hint(&mut tools), "an unrelated Latin-1 properties file keeps web absence checks on");
        let project = host::projects().unwrap().remove(0);
        assert_eq!(tools.absence_note(&project, "http"), None);
        host.add_file_bytes("src/main/java/com/acme/Legacy.java", b"class caf\xe9 {}");
        tools.live_events(&[Event::FileSaved("/project/src/main/java/com/acme/Legacy.java".into())]);
        assert!(!has_hint(&mut tools), "an unreadable source file turns the claim off");
        assert_eq!(tools.absence_note(&project, "http").as_deref(), Some("absence checks off: 1 unreadable file"));
    }

    #[test]
    fn deleted_in_batch_is_removal_not_reset() {
        let (host, mut tools) = setup();
        host.add_file("src/main/java/com/acme/Users.java", "@RestController class Users { @GetMapping(\"/u\") String u() { return null; } }");
        tools.live_events(&[Event::FileSaved("/project/src/main/java/com/acme/Users.java".into())]);
        let project = host::projects().unwrap().remove(0);
        assert_eq!(tools.web.endpoints.get(&project, Duration::ZERO).files().len(), 1);
        let gone = openidea_plugin::FilesChanged {
            created: vec!["/project/src/main/java/com/acme/Gone.java".into()],
            modified: vec!["/project/Gone.properties".into()],
            removed: Vec::new(),
        };
        tools.live_events(&[Event::FilesChanged(gone)]);
        let endpoints = tools.web.endpoints.get(&project, Duration::ZERO);
        assert_eq!(endpoints.files().len(), 1, "the other files stay indexed");
        assert!(endpoints.complete());
        assert!(tools.live.bounds[WEB].get(&project, Duration::ZERO).complete());
    }
    #[test]
    fn wrapper_current_edit_and_fix_returns_one_merged_response_without_manual_publication() {
        let (host, mut tools) = setup();
        let mut d = doc(
            "@Controller class C {@GetMapping(\"/u/{id}\") String get(@PathVariable(\"wrong\") Long id){return null;} @QueryMapping String nope(){return null;}}",
        );
        host.open_editor(&d.path, &d.language, &d.text, &[]);
        host.refresh_diagnostics(&mut tools);
        let problems = host.provider_diagnostics()[&d.path].clone();
        assert_eq!(problems.iter().filter(|d| d.code.as_deref() == Some("unknown-path-variable")).count(), 1);
        assert_eq!(problems.iter().filter(|d| d.code.as_deref() == Some("unknown-schema-field")).count(), 1);
        tools.line_markers(&d);
        assert!(host.diagnostics().is_empty());
        d.version += 1;
        d.text = d.text.replace("wrong", "id").replace("String nope()", "String books()");
        host.open_editor(&d.path, &d.language, &d.text, &[]);
        host.refresh_diagnostics(&mut tools);
        assert!(host.provider_diagnostics().get(&d.path).is_none_or(Vec::is_empty));
    }
    #[test]
    fn scoped_path_hover_uses_exact_imported_owner_and_utf16_range() {
        let (_host, mut tools) = setup();
        let text = "import org.springframework.web.bind.annotation.RestController; import org.springframework.web.bind.annotation.GetMapping; import org.springframework.web.bind.annotation.PathVariable; @RestController class C { /* 😀 */ @GetMapping(\"/u/{id}\") String get(@PathVariable Long id){return null;} }";
        let d = doc(text);
        let from = text.find("{id}").unwrap();
        let at = position(text, from + 1);
        let hover = tools.hover(&d, at).unwrap();
        assert_eq!(hover.range.unwrap(), range(text, from, from + 4));
        let wrong = doc(&text.replace("org.springframework.web.bind.annotation.PathVariable", "other.PathVariable"));
        assert!(tools.hover(&wrong, at).is_none());
        let dynamic = doc(&text.replace("\"/u/{id}\"", "PATH"));
        assert!(tools.hover(&dynamic, position(&dynamic.text, dynamic.text.rfind("id)").unwrap())).is_none());
        let duplicate = doc(&text.replace("Long id)", "Long id, @PathVariable(\"id\") Long second)"));
        assert!(tools.hover(&duplicate, position(&duplicate.text, duplicate.text.find("{id}").unwrap() + 1)).is_none());
    }
    #[test]
    fn merged_java_mapping_and_destination_docs_route_without_discovery_or_runtime_claims(){
        let (h,mut tools)=setup();
        for (text,needle,expected) in [
            ("import org.springframework.stereotype.Controller; import org.springframework.graphql.data.method.annotation.QueryMapping; @Controller class G { @QueryMapping Object books(){return null;} }","@QueryMapping","GraphQL @QueryMapping mapping source"),
            ("import org.springframework.jms.annotation.JmsListener; class L { @JmsListener(destination=\"orders\") void receive(String s){} }","orders","JMS destination annotation source"),
        ] {
            let d=doc(text);let at=position(text,text.find(needle).unwrap());let hover=tools.hover(&d,at).unwrap();assert!(hover.markdown.contains(expected));assert!(hover.markdown.contains("not established"));
        }
        assert!(h.process_runs().is_empty());assert_eq!(h.fetches(),0);
    }
    #[test]
    fn graphql_docs_are_current_schema_declarations_not_resolver_claims() {
        let (_host, mut tools) = setup();
        let mut d = Document {
            path: "/project/schema.graphqls".into(),
            language: "graphql".into(),
            version: 3,
            text: "# 😀\ntype Query { book(id: ID!): Book }".into(),
        };
        let at = position(&d.text, d.text.find("book").unwrap());
        let hover = tools.hover(&d, at).unwrap();
        assert_eq!(hover.range.unwrap().start, at);
        d.version += 1;
        d.text = d.text.replace("book", "novel");
        let at = position(&d.text, d.text.find("novel").unwrap());
        assert!(tools.hover(&d, at).is_some());
    }
    #[test]
    fn messaging_docs_require_exact_xml_namespace_and_literal_destination() {
        let (_host, mut tools) = setup();
        let mut d = Document {
            path: "/project/listeners.xml".into(),
            language: "xml".into(),
            version: 1,
            text: r#"<j:listener xmlns:j="http://www.springframework.org/schema/jms" destination="orders" ref="handler"/>"#.into(),
        };
        let at = position(&d.text, d.text.find("orders").unwrap());
        assert!(tools.hover(&d, at).is_some());
        d.text = d.text.replace("http://www.springframework.org/schema/jms", "urn:other/schema/jms");
        assert!(tools.hover(&d, position(&d.text, d.text.find("orders").unwrap())).is_none());
        d.text = d.text.replace("urn:other/schema/jms", "http://www.springframework.org/schema/jms").replace("orders", "${orders}");
        assert!(tools.hover(&d, position(&d.text, d.text.find("orders").unwrap())).is_none());
    }
    #[test]
    fn cold_oversized_and_malformed_facts_never_prove_schema_absence() {
        let host = MockHost::install();
        host.add_file("pom.xml", POM);
        let mut tools = Endpoints::default();
        let d = doc("@Controller class C {@QueryMapping String missing(){return null;}}");
        assert!(tools.diagnostics(&d).is_empty());
        host.add_file("Huge.java", &" ".repeat(MAX_TEXT + 1));
        tools.activate_live();
        assert!(tools.diagnostics(&d).iter().all(|d| d.code.as_deref() != Some("unknown-schema-field")));
        let mut broken = d.clone();
        broken.text.pop();
        assert!(tools.diagnostics(&broken).is_empty());
        broken.text = " ".repeat(MAX_TEXT + 1);
        assert!(tools.diagnostics(&broken).is_empty());
    }
    #[test]
    fn wrapper_keeps_other_embedded_checks_and_settings_without_marker_duplicates() {
        let (host, mut tools) = setup();
        host.add_file("pom.xml", format!("{POM}<module>child</module>").as_str());
        tools.live_events(&[Event::FileSaved("/project/pom.xml".into())]);
        let xml = Document {
            path: "/project/ws.xml".into(),
            language: "xml".into(),
            version: 1,
            text: r#"<w:message-broker xmlns:w="http://www.springframework.org/schema/websocket"><w:stomp-error-handler/></w:message-broker>"#
                .into(),
        };
        assert!(tools.diagnostics(&xml).iter().any(|d| d.code.as_deref() == Some("missing-attribute")));
        let flow = Document {
            path: "/project/flow.xml".into(),
            language: "xml".into(),
            version: 1,
            text: r#"<flow xmlns="http://www.springframework.org/schema/webflow"><view-state id="same"/><view-state id="same"/></flow>"#
                .into(),
        };
        assert!(tools.diagnostics(&flow).iter().any(|d| d.code.as_deref() == Some("duplicate-state")));
        let feign =
            doc("import org.springframework.cloud.openfeign.FeignClient; @FeignClient(name=\"api\") interface Api {String fetch();}");
        assert!(tools.diagnostics(&feign).iter().any(|d| d.code.as_deref() == Some("feign-method-without-mapping")));
        let d = doc("@Controller class C {@QueryMapping String missing(){return null;}}");
        assert!(tools.diagnostics(&d).iter().any(|d| d.code.as_deref() == Some("unknown-schema-field")));
        host.set_setting("springGraphql.checkUnmatchedMapping", "false");
        assert!(tools.diagnostics(&d).iter().all(|d| d.code.as_deref() != Some("unknown-schema-field")));
        tools.line_markers(&xml);
        tools.line_markers(&flow);
        tools.line_markers(&feign);
        assert!(host.diagnostics().is_empty());
    }
    #[test]
    fn unsaved_handler_snapshot_updates_later_schema_checks() {
        let (_host, mut tools) = setup();
        let mut controller = doc("@Controller class C {@QueryMapping String missing(){return null;}}");
        let schema = Document {
            path: "/project/src/main/resources/graphql/schema.graphqls".into(),
            language: "graphql".into(),
            version: 2,
            text: "type Query { books: [String] }".into(),
        };
        tools.diagnostics(&controller);
        assert!(tools.diagnostics(&schema).iter().any(|d| d.code.as_deref() == Some("unmapped-root-field")));
        controller.version += 1;
        controller.text = controller.text.replace("missing", "books");
        tools.diagnostics(&controller);
        assert!(tools.diagnostics(&schema).iter().all(|d| d.code.as_deref() != Some("unmapped-root-field")));
    }
}
