//! Spring Web Services Tools for OpenIDEA: links `@PayloadRoot` endpoint
//! methods to the top-level elements of the project's XML schemas (`.xsd`
//! files and schemas embedded in `.wsdl`) and checks the classpath resources
//! given to `ClassPathResource`. Text-based, Java only.

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::{
    Diagnostic, Error, Invocation, Location, Plugin, Position, Range, Severity, host, register_plugin,
};
use spring_text::index::{Index, Indexes, PROVIDER_BUDGET, project_of};
use spring_text::java::{self, Arg, JavaFile, Str};
use spring_text::{SOURCE_GLOBS, contains, parse_source, range, source_calls, xml};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    /// `None` when the namespace isn't a literal or a constant of the file.
    pub namespace: Option<String>,
    pub local: String,
    /// The `localPart` literal.
    pub local_range: Range,
    /// `Class.method`.
    pub label: String,
    /// The method name.
    pub target: Range,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schema {
    pub namespace: String,
    /// Top-level `element` names with the range of their `name` value.
    pub elements: Vec<(String, Range)>,
}

/// The value of an annotation argument that is a string literal or a string
/// constant of the same file (`NAMESPACE_URI`, `Ns.URI`).
fn constant(f: &JavaFile, text: &str, arg: &Arg) -> Option<String> {
    let literal = |code: &str| {
        let toks = java::tokens(code);
        match toks.as_slice() {
            [t] => t.string().map(|s| s.value),
            _ => None,
        }
    };
    if let Some(value) = literal(&arg.text) {
        return Some(value);
    }
    let name = arg.text.rsplit('.').next()?.trim();
    let field = f.types.iter().flat_map(|t| t.fields()).find(|m| m.name == name)?;
    let (from, to) = field.body?;
    literal(&text[from..to])
}

pub fn endpoints(f: &JavaFile, text: &str) -> Vec<Endpoint> {
    let mut out = Vec::new();
    for t in f.types.iter().filter(|t| t.has(&["Endpoint"])) {
        for m in t.methods() {
            for a in m.annotations.iter().filter(|a| a.name == "PayloadRoot") {
                let Some(local) = a.strings(&["localPart"]).first().copied() else { continue };
                out.push(Endpoint {
                    namespace: a.arg("namespace").and_then(|arg| constant(f, text, arg)),
                    local: local.value.clone(),
                    local_range: range(text, local.at, local.end),
                    label: format!("{}.{}", t.name, m.name),
                    target: range(text, m.at, m.at + m.name.len()),
                });
            }
        }
    }
    out
}

pub fn schemas(text: &str) -> Vec<Schema> {
    let elements = xml::elements(text);
    elements
        .iter()
        .enumerate()
        .filter(|(_, e)| e.local() == "schema")
        .map(|(i, e)| Schema {
            namespace: e.value("targetNamespace").unwrap_or_default().to_owned(),
            elements: elements
                .iter()
                .filter(|c| c.parent == Some(i) && c.local() == "element")
                .filter_map(|c| c.attr("name"))
                .map(|a| (a.value.clone(), range(text, a.value_at, a.value_end)))
                .collect(),
        })
        .collect()
}

fn scan_java(path: &str, text: &str) -> Option<Vec<Endpoint>> {
    let found = endpoints(&parse_source(path, text), text);
    (!found.is_empty()).then_some(found)
}

/// Every schema file is kept, even without a schema: it tells resources exist.
fn scan_schemas(_path: &str, text: &str) -> Option<Vec<Schema>> {
    Some(schemas(text))
}

/// The `ClassPathResource` paths of a file, `classpath:` and leading `/` removed.
fn resources(path: &str, text: &str) -> Vec<(String, Str)> {
    source_calls(path, text)
        .into_iter()
        .filter(|c| c.is_new && c.name == "ClassPathResource")
        .filter_map(|c| c.args.into_iter().next()?.string)
        .map(|s| (s.value.trim_start_matches("classpath:").trim_start_matches('/').to_owned(), s))
        .filter(|(path, _)| !path.is_empty() && !path.contains("${"))
        .collect()
}

const RESOURCE_DIRS: [&str; 2] = ["src/main/resources", "src/test/resources"];

/// The schema elements `e` may name: in the schemas of its namespace, or in
/// every schema when the namespace is unknown or has no schema.
fn elements_for<'a>(schemas: &'a Index<Vec<Schema>>, namespace: Option<&str>) -> Vec<(&'a str, &'a str, Range)> {
    let all: Vec<(&str, &Schema)> = schemas.iter().flat_map(|(p, s)| s.iter().map(move |s| (p, s))).collect();
    let of_namespace: Vec<_> = all.iter().filter(|(_, s)| Some(s.namespace.as_str()) == namespace).copied().collect();
    let chosen = if of_namespace.is_empty() { all } else { of_namespace };
    chosen.into_iter().flat_map(|(p, s)| s.elements.iter().map(move |(n, r)| (p, n.as_str(), *r))).collect()
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

pub struct WsTools {
    java: Indexes<Vec<Endpoint>>,
    schemas: Indexes<Vec<Schema>>,
}

impl Default for WsTools {
    fn default() -> Self {
        Self { java: Indexes::new(SOURCE_GLOBS, scan_java), schemas: Indexes::new(&["**/*.xsd", "**/*.wsdl"], scan_schemas) }
    }
}

impl WsTools {
    fn java_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let text = &doc.text;
        let f = parse_source(&doc.path, text);
        let Some(schemas) = self.schemas.for_document(&doc.path, text, PROVIDER_BUDGET) else { return Vec::new() };
        let mut markers = Vec::new();
        let mut problems = Vec::new();
        for e in endpoints(&f, text) {
            let found = elements_for(schemas, e.namespace.as_deref());
            match found.iter().find(|(_, n, _)| *n == e.local) {
                Some((path, _, r)) => markers.push(LineMarker {
                    line: e.target.start.line,
                    icon: None,
                    tooltip: format!("Payload element <{}> in {}", e.local, file_name(path)),
                    target: Some(Location { path: (*path).into(), range: *r }),
                }),
                None if schemas.complete()
                    && let Some(ns) = &e.namespace
                    && schemas.iter().flat_map(|(_, s)| s).any(|s| &s.namespace == ns) =>
                {
                    let message = format!("No schema of namespace {ns} has a top-level element `{}`.", e.local);
                    problems.push(Diagnostic::new(e.local_range, Severity::Warning, message).code("unknown-payload-element"));
                }
                None => {}
            }
        }
        if schemas.complete() {
            for (path, s) in resources(&doc.path, text) {
                let schema_file = path.ends_with(".xsd") || path.ends_with(".wsdl");
                let exists = schemas.files().keys().any(|p| RESOURCE_DIRS.iter().any(|d| p.ends_with(&format!("{d}/{path}"))));
                if schema_file && !exists {
                    let message = format!("`{path}` is not in src/main/resources or src/test/resources.");
                    problems.push(Diagnostic::new(range(text, s.at, s.end), Severity::Warning, message).code("missing-resource"));
                }
            }
        }
        for b in java::bean_names(&f).into_iter().filter(|b| b.ty == "DefaultWsdl11Definition") {
            let line = openidea_plugin::text::position(text, b.at).line;
            markers.push(LineMarker { line, icon: None, tooltip: format!("Published as /{}.wsdl", b.name), target: None });
        }
        let _ = host::publish_diagnostics(&doc.path, &problems);
        markers
    }

    fn xml_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let found = schemas(&doc.text);
        if found.iter().all(|s| s.elements.is_empty()) {
            return Vec::new();
        }
        let Some(java) = self.java.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else { return Vec::new() };
        let mut markers = Vec::new();
        for s in &found {
            for (name, r) in &s.elements {
                let handlers: Vec<(&str, &Endpoint)> = java
                    .iter()
                    .flat_map(|(p, es)| es.iter().map(move |e| (p, e)))
                    .filter(|(_, e)| &e.local == name && e.namespace.as_ref().is_none_or(|ns| ns == &s.namespace))
                    .collect();
                if let Some((path, e)) = handlers.first() {
                    let labels: Vec<&str> = handlers.iter().map(|(_, e)| e.label.as_str()).collect();
                    markers.push(LineMarker {
                        line: r.start.line,
                        icon: None,
                        tooltip: format!("Handled by {}", labels.join(", ")),
                        target: Some(Location { path: (*path).into(), range: e.target }),
                    });
                }
            }
        }
        markers
    }
}

impl Providers for WsTools {
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        if matches!(doc.language.as_str(), "java" | "kotlin") { self.java_markers(doc) } else { self.xml_markers(doc) }
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let text = &doc.text;
        let endpoint = endpoints(&parse_source(&doc.path, text), text).into_iter().find(|e| contains(e.local_range, at));
        if let Some(e) = endpoint {
            let Some(schemas) = self.schemas.for_document(&doc.path, text, PROVIDER_BUDGET) else { return Vec::new() };
            return elements_for(schemas, e.namespace.as_deref())
                .into_iter()
                .filter(|(_, n, _)| *n == e.local)
                .map(|(p, _, r)| Location { path: p.into(), range: r })
                .collect();
        }
        let Some((path, _)) = resources(&doc.path, text).into_iter().find(|(_, s)| contains(range(text, s.at, s.end), at)) else {
            return Vec::new();
        };
        let Some(project) = project_of(&doc.path) else { return Vec::new() };
        RESOURCE_DIRS
            .iter()
            .flat_map(|d| project.find_files(&format!("**/{d}/{path}"), 1).unwrap_or_default())
            .map(|p| Location { path: p, range: Range::default() })
            .collect()
    }

    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let text = &doc.text;
        let Some(e) = endpoints(&parse_source(&doc.path, text), text).into_iter().find(|e| contains(e.local_range, at)) else {
            return Vec::new();
        };
        let Some(schemas) = self.schemas.for_document(&doc.path, text, PROVIDER_BUDGET) else { return Vec::new() };
        let mut out: Vec<CompletionItem> = elements_for(schemas, e.namespace.as_deref())
            .into_iter()
            .map(|(p, n, _)| CompletionItem {
                detail: Some(file_name(p).into()),
                replace: Some(e.local_range),
                ..CompletionItem::new(n, CompletionKind::Struct)
            })
            .collect();
        out.sort_by(|a, b| a.label.cmp(&b.label));
        out.dedup_by(|a, b| a.label == b.label);
        out
    }
}

impl Plugin for WsTools {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

register_plugin!(WsTools, providers);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::text::position;

    const ENDPOINT: &str = r#"package com.acme.ws;

@Endpoint
public class CountryEndpoint {
    private static final String NAMESPACE_URI = "http://acme.com/countries";

    @PayloadRoot(namespace = NAMESPACE_URI, localPart = "getCountryRequest")
    @ResponsePayload
    public GetCountryResponse getCountry(@RequestPayload GetCountryRequest request) { return null; }

    @PayloadRoot(namespace = NAMESPACE_URI, localPart = "deleteCountryRequest")
    public void delete(@RequestPayload Object request) {}

    @PayloadRoot(namespace = "http://acme.com/orders", localPart = "placeOrder")
    public void order(@RequestPayload Object request) {}
}

@Configuration
class WsConfig {
    @Bean(name = "countries")
    public DefaultWsdl11Definition defaultWsdl11Definition(XsdSchema countriesSchema) { return null; }

    @Bean
    public XsdSchema countriesSchema() { return new SimpleXsdSchema(new ClassPathResource("countries.xsd")); }

    @Bean
    public XsdSchema missing() { return new SimpleXsdSchema(new ClassPathResource("/xsd/missing.xsd")); }
}
"#;

    const XSD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="http://acme.com/countries">
    <xs:element name="getCountryRequest">
        <xs:complexType><xs:sequence><xs:element name="name" type="xs:string"/></xs:sequence></xs:complexType>
    </xs:element>
    <xs:element name="getCountryResponse"/>
</xs:schema>
"#;

    const WSDL: &str = r#"<wsdl:definitions xmlns:wsdl="http://schemas.xmlsoap.org/wsdl/">
  <wsdl:types>
    <schema xmlns="http://www.w3.org/2001/XMLSchema" targetNamespace="http://acme.com/orders">
      <element name="placeOrder"/>
    </schema>
  </wsdl:types>
</wsdl:definitions>
"#;

    const JAVA_PATH: &str = "/project/src/main/java/com/acme/ws/CountryEndpoint.java";
    const XSD_PATH: &str = "/project/src/main/resources/countries.xsd";

    fn setup() -> MockHost {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/ws/CountryEndpoint.java", ENDPOINT);
        host.add_file("src/main/resources/countries.xsd", XSD);
        host.add_file("src/main/resources/wsdl/orders.wsdl", WSDL);
        host
    }

    fn java() -> Document {
        Document { path: JAVA_PATH.into(), language: "java".into(), version: 1, text: ENDPOINT.into() }
    }

    fn pos(text: &str, needle: &str) -> Position {
        position(text, text.find(needle).unwrap())
    }

    const KOTLIN_ENDPOINT: &str = r#"package com.acme.ws

@Endpoint
class KtCountryEndpoint {
    @PayloadRoot(namespace = NAMESPACE_URI, localPart = "getCountryRequest")
    @ResponsePayload
    suspend fun getCountry(@RequestPayload request: GetCountryRequest): GetCountryResponse = TODO()

    @PayloadRoot(namespace = NAMESPACE_URI, localPart = "ghostRequest")
    fun ghost(@RequestPayload request: Any) {}

    companion object {
        const val NAMESPACE_URI = "http://acme.com/countries"
    }
}
"#;

    #[test]
    fn kotlin_payload_root_is_indexed() {
        let kt = parse_source("K.kt", KOTLIN_ENDPOINT);
        let found = endpoints(&kt, KOTLIN_ENDPOINT);
        let names: Vec<_> = found.iter().map(|e| (e.namespace.as_deref().unwrap(), e.local.as_str())).collect();
        assert_eq!(names, [("http://acme.com/countries", "getCountryRequest"), ("http://acme.com/countries", "ghostRequest")]);
        assert_eq!(scan_java("K.kt", KOTLIN_ENDPOINT).unwrap().len(), 2);
        assert_eq!(endpoints(&java::parse(ENDPOINT), ENDPOINT).len(), 3);

        let host = setup();
        let path = "/project/src/main/kotlin/com/acme/ws/KtCountryEndpoint.kt";
        host.add_file("src/main/kotlin/com/acme/ws/KtCountryEndpoint.kt", KOTLIN_ENDPOINT);
        let mut ws = WsTools::default();
        let doc = Document { path: path.into(), language: "kotlin".into(), version: 1, text: KOTLIN_ENDPOINT.into() };
        let tips: Vec<_> = ws.line_markers(&doc).into_iter().map(|m| m.tooltip).collect();
        assert_eq!(tips, ["Payload element <getCountryRequest> in countries.xsd"]);
        let problems = &host.diagnostics()[path];
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code.as_deref(), Some("unknown-payload-element"));
        let xsd_tips: Vec<_> = ws
            .line_markers(&Document { path: XSD_PATH.into(), language: "xml".into(), version: 1, text: XSD.into() })
            .into_iter()
            .map(|m| m.tooltip)
            .collect();
        assert!(xsd_tips[0].contains("KtCountryEndpoint.getCountry"), "{xsd_tips:?}");
    }

    #[test]
    fn reads_endpoints_and_schemas() {
        let found = endpoints(&java::parse(ENDPOINT), ENDPOINT);
        let names: Vec<_> = found.iter().map(|e| (e.namespace.as_deref().unwrap(), e.local.as_str())).collect();
        assert_eq!(names, [
            ("http://acme.com/countries", "getCountryRequest"),
            ("http://acme.com/countries", "deleteCountryRequest"),
            ("http://acme.com/orders", "placeOrder")
        ]);
        let s = schemas(XSD);
        assert_eq!(s[0].namespace, "http://acme.com/countries");
        assert_eq!(s[0].elements.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["getCountryRequest", "getCountryResponse"]);
        assert_eq!(schemas(WSDL)[0].elements[0].0, "placeOrder");
    }

    #[test]
    fn links_endpoints_to_schema_elements_and_checks_them() {
        let host = setup();
        let mut ws = WsTools::default();
        let markers = ws.line_markers(&java());
        let tips: Vec<_> = markers.iter().map(|m| (m.line, m.tooltip.as_str())).collect();
        assert_eq!(tips, [
            (8, "Payload element <getCountryRequest> in countries.xsd"),
            (14, "Payload element <placeOrder> in orders.wsdl"),
            (20, "Published as /countries.wsdl"),
        ]);
        assert_eq!(markers[0].target.as_ref().unwrap().path, XSD_PATH);
        assert_eq!(markers[0].target.as_ref().unwrap().range.start, pos(XSD, "getCountryRequest\""));

        let problems = &host.diagnostics()[JAVA_PATH];
        let codes: Vec<_> = problems.iter().map(|p| (p.code.as_deref().unwrap(), p.range.start)).collect();
        assert_eq!(codes, [
            ("unknown-payload-element", pos(ENDPOINT, "deleteCountryRequest")),
            ("missing-resource", pos(ENDPOINT, "/xsd/missing.xsd"))
        ]);
    }

    #[test]
    fn navigates_and_completes_payload_elements() {
        let _host = setup();
        let mut ws = WsTools::default();
        let found = ws.definition(&java(), pos(ENDPOINT, "etCountryRequest"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, XSD_PATH);

        let labels: Vec<_> = ws.completion(&java(), pos(ENDPOINT, "eleteCountryRequest"), None).into_iter().map(|i| i.label).collect();
        assert_eq!(labels, ["getCountryRequest", "getCountryResponse"]);
    }

    #[test]
    fn navigates_to_classpath_resources() {
        let _host = setup();
        let mut ws = WsTools::default();
        let found = ws.definition(&java(), pos(ENDPOINT, "ountries.xsd"));
        assert_eq!(found, [Location { path: XSD_PATH.into(), range: Range::default() }]);
        assert!(ws.definition(&java(), pos(ENDPOINT, "sd/missing.xsd")).is_empty());
    }

    #[test]
    fn marks_schema_elements_with_their_endpoint() {
        let _host = setup();
        let mut ws = WsTools::default();
        let doc = Document { path: XSD_PATH.into(), language: "xml".into(), version: 1, text: XSD.into() };
        let markers = ws.line_markers(&doc);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].line, 2);
        assert_eq!(markers[0].tooltip, "Handled by CountryEndpoint.getCountry");
        assert_eq!(markers[0].target.as_ref().unwrap().range.start, pos(ENDPOINT, "getCountry("));
    }
}
