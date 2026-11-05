//! Spring Integration Tools for OpenIDEA: indexes the message channels a
//! project declares and the endpoints that read from or send to them (Java
//! annotations, the Java DSL and integration XML), then links producers,
//! channels and consumers. Text-based: channels match by name as written.

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::{Diagnostic, Error, Invocation, Location, Plugin, Position, Range, Severity, host, register_plugin};
use spring_text::index::{Index, Indexes, PROVIDER_BUDGET};
use spring_text::java::{Annotation, Str};
use spring_text::{beans, contains, range, xml};
use std::collections::BTreeSet;

const NAMESPACE: &str = "http://www.springframework.org/schema/integration";
const ENDPOINTS: [&str; 12] = [
    "ServiceActivator",
    "Transformer",
    "Filter",
    "Router",
    "Splitter",
    "Aggregator",
    "BridgeFrom",
    "BridgeTo",
    "InboundChannelAdapter",
    "Publisher",
    "MessagingGateway",
    "Gateway",
];
/// `MessageChannels` factory methods that name the channel they create.
const CHANNEL_FACTORIES: [&str; 7] = ["direct", "queue", "publishSubscribe", "executor", "flux", "priority", "rendezvous"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Creates the channel: a channel `@Bean`, `MessageChannels.queue("x")`, `<int:channel id>`, DSL `.channel("x")`.
    Declares,
    /// An endpoint reading from the channel.
    Consumes,
    /// An endpoint sending to it.
    Produces,
    /// A bean of any kind declared under this name.
    Bean,
    /// An XML `ref` to a bean.
    BeanRef,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ref {
    pub name: String,
    pub role: Role,
    /// Where it is written (`@ServiceActivator handle()`, `<int:bridge>`).
    pub what: String,
    pub range: Range,
    /// Reported when nothing declares or consumes the channel.
    pub checked: bool,
}

/// The channel and bean names of a Java, Kotlin or XML source.
pub fn parse(path: &str, text: &str) -> Vec<Ref> {
    if path.ends_with(".xml") { parse_xml(text) } else { parse_source(path, text) }
}

fn index_file(path: &str, text: &str) -> Option<Vec<Ref>> {
    Some(parse(path, text)).filter(|refs| !beans::ignored(path) && !refs.is_empty())
}

fn str_ref(text: &str, s: &Str, role: Role, what: String, checked: bool) -> Ref {
    Ref { name: s.value.clone(), role, what, range: range(text, s.at, s.end), checked }
}

fn annotation_refs(text: &str, a: &Annotation, what: &str, out: &mut Vec<Ref>) {
    for arg in &a.args {
        let role = match (a.name.as_str(), arg.name.as_str()) {
            ("BridgeFrom", "value") => Role::Consumes,
            ("BridgeTo" | "InboundChannelAdapter" | "Publisher", "value") => Role::Produces,
            (_, "inputChannel" | "replyChannel" | "defaultReplyChannel") => Role::Consumes,
            (_, n) if n == "channel" || n.ends_with("Channel") => Role::Produces,
            _ => continue,
        };
        for s in &arg.strings {
            out.push(str_ref(text, s, role, format!("@{} {what}", a.name), role == Role::Produces));
        }
    }
}

fn parse_source(path: &str, text: &str) -> Vec<Ref> {
    let file = spring_text::parse_source(path, text);
    let mut out = Vec::new();
    for b in spring_text::java::bean_names(&file) {
        let r = range(text, b.at, b.at + b.len);
        let what = format!("{} bean", b.ty);
        if b.ty.ends_with("Channel") {
            out.push(Ref { name: b.name.clone(), role: Role::Declares, what: what.clone(), range: r, checked: false });
        }
        out.push(Ref { name: b.name, role: Role::Bean, what, range: r, checked: false });
    }
    if !text.contains("integration") {
        return out;
    }
    for t in &file.types {
        for a in t.annotations.iter().filter(|a| a.name == "MessagingGateway") {
            annotation_refs(text, a, &t.name, &mut out);
        }
        for m in t.methods() {
            for a in m.annotations.iter().filter(|a| ENDPOINTS.contains(&a.name.as_str())) {
                annotation_refs(text, a, &format!("{}()", m.name), &mut out);
            }
        }
    }
    if !text.contains("IntegrationFlow") && !text.contains("MessageChannels") {
        return out;
    }
    let calls = spring_text::source_calls(path, text);
    let in_channel_call =
        |at: usize| calls.iter().any(|c| c.name == "channel" && c.args.first().is_some_and(|a| a.at <= at && at < a.end));
    for c in &calls {
        let Some(s) = c.args.first().and_then(|a| a.string.as_ref()) else { continue };
        let receiver = c.receiver.as_deref();
        match c.name.as_str() {
            n if CHANNEL_FACTORIES.contains(&n) && (receiver == Some("MessageChannels") || in_channel_call(c.at)) => {
                out.push(str_ref(text, s, Role::Declares, format!("MessageChannels.{n}"), false));
            }
            "from" if matches!(receiver, Some("IntegrationFlow" | "IntegrationFlows")) => {
                out.push(str_ref(text, s, Role::Consumes, "IntegrationFlow.from".into(), false));
            }
            // The DSL creates the channel when no bean has that name, so it is never unresolved.
            "channel" if !c.is_new && text.contains("IntegrationFlow") => {
                out.push(str_ref(text, s, Role::Declares, "flow .channel".into(), false));
                out.push(str_ref(text, s, Role::Produces, "flow .channel".into(), false));
            }
            _ => {}
        }
    }
    out
}

fn parse_xml(text: &str) -> Vec<Ref> {
    if !text.contains("www.springframework.org/schema/") {
        return Vec::new();
    }
    let els = xml::elements(text);
    let attr_range = |a: &xml::Attr| range(text, a.value_at, a.value_end);
    let mut out: Vec<Ref> = xml::bean_ids(&els)
        .into_iter()
        .map(|a| Ref { name: a.value.clone(), role: Role::Bean, what: "XML bean".into(), range: attr_range(a), checked: false })
        .collect();
    if !text.contains(NAMESPACE) {
        return out;
    }
    for (i, e) in els.iter().enumerate() {
        if !xml::namespace(&els, i).is_some_and(|ns| ns.starts_with(NAMESPACE)) {
            continue;
        }
        let outbound = e.local().contains("outbound");
        for a in &e.attrs {
            let role = match a.local() {
                "id" if matches!(e.local(), "channel" | "publish-subscribe-channel") => Role::Declares,
                "input-channel" | "default-reply-channel" => Role::Consumes,
                "reply-channel" if !outbound => Role::Consumes,
                "request-channel" | "channel" if outbound => Role::Consumes,
                "output-channel" | "default-output-channel" | "discard-channel" | "error-channel"
                | "default-request-channel" | "request-channel" | "reply-channel" | "channel" => Role::Produces,
                "ref" => Role::BeanRef,
                _ => continue,
            };
            let checked = matches!(role, Role::Produces | Role::BeanRef);
            out.push(Ref { name: a.value.clone(), role, what: format!("<{}>", e.name), range: attr_range(a), checked });
        }
    }
    out
}

/// Whether a document may hold channels or bean references worth looking at.
fn relevant(doc: &Document) -> bool {
    match doc.language.as_str() {
        "xml" => doc.text.contains(NAMESPACE),
        _ => doc.text.contains("integration"),
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

pub struct Integration {
    indexes: Indexes<Vec<Ref>>,
}

impl Default for Integration {
    fn default() -> Self {
        Self { indexes: Indexes::new(&beans::GLOBS, index_file) }
    }
}

/// Every reference of the project with its file.
fn all(index: &Index<Vec<Ref>>) -> impl Iterator<Item = (&str, &Ref)> {
    index.iter().flat_map(|(path, refs)| refs.iter().map(move |r| (path, r)))
}

/// The references named like `r` in `roles`, `r` itself excluded.
fn others<'a>(index: &'a Index<Vec<Ref>>, doc: &Document, r: &Ref, roles: &[Role]) -> Vec<(&'a str, &'a Ref)> {
    all(index)
        .filter(|(p, x)| roles.contains(&x.role) && x.name == r.name && !(*p == doc.path && x.range == r.range))
        .collect()
}

fn location((path, r): (&str, &Ref)) -> Location {
    Location { path: path.into(), range: r.range }
}

impl Integration {
    fn index(&mut self, doc: &Document) -> Option<&Index<Vec<Ref>>> {
        if !relevant(doc) {
            return None;
        }
        self.indexes.for_document(&doc.path, &doc.text, PROVIDER_BUDGET).map(|i| &*i)
    }
}

fn here<'a>(index: &'a Index<Vec<Ref>>, doc: &Document) -> &'a [Ref] {
    index.files().get(&doc.path).map_or(&[], |refs| refs.as_slice())
}

fn problems(index: &Index<Vec<Ref>>, doc: &Document) -> Vec<Diagnostic> {
    if !index.complete() {
        return Vec::new();
    }
    let known = |name: &str, roles: &[Role]| all(index).any(|(_, x)| x.name == name && roles.contains(&x.role));
    here(index, doc)
        .iter()
        .filter(|r| r.checked && !r.name.is_empty() && !beans::is_expression(&r.name))
        .filter(|r| !beans::INFRASTRUCTURE.contains(&r.name.as_str()))
        .filter_map(|r| {
            let (code, message) = match r.role {
                Role::BeanRef if !known(&r.name, &[Role::Bean]) => {
                    ("unknown-bean", format!("No bean named '{}' is declared in the project.", r.name))
                }
                Role::Produces if !known(&r.name, &[Role::Declares, Role::Consumes, Role::Bean]) => (
                    "unconsumed-channel",
                    format!("No endpoint consumes channel '{}' and no bean declares it, so nothing receives the messages sent to it.", r.name),
                ),
                _ => return None,
            };
            Some(Diagnostic::new(r.range, Severity::Warning, message).code(code))
        })
        .collect()
}

impl Providers for Integration {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(index) = self.index(doc) else { return Vec::new() };
        let Some(r) = here(index, doc)
            .iter()
            .find(|r| contains(r.range, at) && matches!(r.role, Role::Consumes | Role::Produces | Role::BeanRef))
        else {
            return Vec::new();
        };
        let (roles, detail) = match r.role {
            Role::BeanRef => (&[Role::Bean][..], "bean"),
            _ => (&[Role::Declares, Role::Consumes, Role::Produces][..], "message channel"),
        };
        let names: BTreeSet<&str> = all(index)
            .filter(|(_, x)| roles.contains(&x.role) && !x.name.is_empty() && !beans::is_expression(&x.name))
            .map(|(_, x)| x.name.as_str())
            .collect();
        names
            .into_iter()
            .map(|name| CompletionItem {
                detail: Some(detail.into()),
                replace: Some(r.range),
                ..CompletionItem::new(name, CompletionKind::Reference)
            })
            .collect()
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let Some(index) = self.index(doc) else { return Vec::new() };
        let Some(r) = here(index, doc).iter().find(|r| contains(r.range, at) && r.role != Role::Bean && !r.name.is_empty())
        else {
            return Vec::new();
        };
        let found = match r.role {
            Role::BeanRef => others(index, doc, r, &[Role::Bean]),
            _ => Some(others(index, doc, r, &[Role::Declares]))
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| others(index, doc, r, &[Role::Consumes])),
        };
        found.into_iter().map(location).collect()
    }

    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let Some(index) = self.index(doc) else { return Vec::new() };
        let _ = host::publish_diagnostics(&doc.path, &problems(index, doc));
        let mut markers = Vec::new();
        for r in here(index, doc).iter().filter(|r| !r.name.is_empty()) {
            let (found, title) = match r.role {
                Role::Consumes => (others(index, doc, r, &[Role::Produces]), format!("Sent to '{}' by", r.name)),
                Role::Produces => (others(index, doc, r, &[Role::Consumes]), format!("'{}' is consumed by", r.name)),
                _ => continue,
            };
            let Some(&first) = found.first() else { continue };
            let list: Vec<String> = found
                .iter()
                .map(|(p, x)| format!("{} ({}:{})", x.what, file_name(p), x.range.start.line + 1))
                .collect();
            markers.push(LineMarker {
                line: r.range.start.line,
                icon: None,
                tooltip: format!("{title}: {}", list.join(", ")),
                target: Some(location(first)),
            });
        }
        markers
    }
}

impl Plugin for Integration {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

register_plugin!(Integration, providers);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    const CONFIG: &str = r#"package com.acme;

import org.springframework.integration.dsl.IntegrationFlow;
import org.springframework.integration.dsl.MessageChannels;

@Configuration
public class FlowConfig {
    @Bean
    public MessageChannel orders() { return new DirectChannel(); }

    @Bean
    public IntegrationFlow shipping() {
        return IntegrationFlow.from("orders")
            .channel("shipping")
            .channel(c -> c.queue("audit"))
            .get();
    }

    @Bean
    public PollableChannel late() { return MessageChannels.priority("late").getObject(); }
}
"#;
    const ENDPOINTS_SRC: &str = r#"package com.acme;

import org.springframework.integration.annotation.*;

@Component
public class Billing {
    @ServiceActivator(inputChannel = "shipping", outputChannel = "invoices")
    public Invoice bill(Order order) { return null; }

    @Transformer(inputChannel = "audit", outputChannel = "nowhere")
    public String audit(Order order) { return ""; }

    @Filter(inputChannel = "late", discardChannel = "${discards}")
    public boolean accept(Order order) { return true; }
}

@MessagingGateway(defaultRequestChannel = "orders", errorChannel = "errorChannel")
interface OrderGateway {
    @Gateway(requestChannel = "orders", replyChannel = "confirmations")
    void place(Order order);
}
"#;
    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<beans xmlns="http://www.springframework.org/schema/beans"
       xmlns:int="http://www.springframework.org/schema/integration"
       xmlns:int-http="http://www.springframework.org/schema/integration/http">
    <int:channel id="invoices"/>
    <int:service-activator input-channel="invoices" output-channel="lost" ref="billing"/>
    <int:outbound-channel-adapter channel="invoices" ref="mailer"/>
    <int-http:outbound-gateway request-channel="quotes" reply-channel="quoted" url="http://x"/>
    <bean id="pricing" class="com.acme.Pricing"/>
</beans>
"#;
    const CONFIG_PATH: &str = "/project/src/main/java/com/acme/FlowConfig.java";
    const BILLING_PATH: &str = "/project/src/main/java/com/acme/Billing.java";
    const XML_PATH: &str = "/project/src/main/resources/integration.xml";

    fn setup() -> MockHost {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/FlowConfig.java", CONFIG);
        host.add_file("src/main/java/com/acme/Billing.java", ENDPOINTS_SRC);
        host.add_file("src/main/resources/integration.xml", XML);
        host.add_file("pom.xml", "<project><int:channel id=\"ignored\"/></project>");
        host
    }

    fn doc(path: &str, text: &str) -> Document {
        let language = if path.ends_with(".xml") { "xml" } else { "java" };
        Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
    }

    fn at(text: &str, needle: &str) -> Position {
        openidea_plugin::text::position(text, text.find(needle).unwrap() + 1)
    }

    fn names(refs: &[Ref], role: Role) -> Vec<&str> {
        refs.iter().filter(|r| r.role == role).map(|r| r.name.as_str()).collect()
    }

    #[test]
    fn reads_channels_from_java_and_xml() {
        let config = parse(CONFIG_PATH, CONFIG);
        assert_eq!(names(&config, Role::Declares), ["orders", "late", "shipping", "audit", "late"]);
        assert_eq!(names(&config, Role::Consumes), ["orders"]);
        assert_eq!(names(&config, Role::Produces), ["shipping"]);

        let billing = parse(BILLING_PATH, ENDPOINTS_SRC);
        assert_eq!(names(&billing, Role::Consumes), ["shipping", "audit", "late", "confirmations"]);
        assert_eq!(names(&billing, Role::Produces), ["invoices", "nowhere", "${discards}", "orders", "errorChannel", "orders"]);

        let xml = parse(XML_PATH, XML);
        assert_eq!(names(&xml, Role::Declares), ["invoices"]);
        assert_eq!(names(&xml, Role::Consumes), ["invoices", "invoices", "quotes"]);
        assert_eq!(names(&xml, Role::Produces), ["lost", "quoted"]);
        assert_eq!(names(&xml, Role::BeanRef), ["billing", "mailer"]);
        assert!(parse(XML_PATH, "<beans><int:channel id=\"x\"/></beans>").is_empty());
    }

    #[test]
    fn goes_to_the_channel_declaration_or_its_consumers() {
        let _host = setup();
        let mut p = Integration::default();
        let found = p.definition(&doc(BILLING_PATH, ENDPOINTS_SRC), at(ENDPOINTS_SRC, "\"shipping\""));
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].path.as_str(), found[0].range.start.line), (CONFIG_PATH, 13));

        let found = p.definition(&doc(BILLING_PATH, ENDPOINTS_SRC), at(ENDPOINTS_SRC, "\"invoices\""));
        assert_eq!((found[0].path.as_str(), found[0].range.start.line), (XML_PATH, 4));

        let found = p.definition(&doc(XML_PATH, XML), at(XML, "\"quoted\""));
        assert!(found.is_empty(), "nothing declares or consumes it");

        let found = p.definition(&doc(XML_PATH, XML), at(XML, "\"billing\""));
        assert_eq!((found[0].path.as_str(), found[0].range.start.line), (BILLING_PATH, 5));
    }

    #[test]
    fn marks_producers_and_consumers() {
        let _host = setup();
        let mut p = Integration::default();
        let markers = p.line_markers(&doc(BILLING_PATH, ENDPOINTS_SRC));
        let tips: Vec<_> = markers.iter().map(|m| (m.line, m.tooltip.as_str())).collect();
        assert_eq!(tips, [
            (6, "Sent to 'shipping' by: flow .channel (FlowConfig.java:14)"),
            (6, "'invoices' is consumed by: <int:service-activator> (integration.xml:6), <int:outbound-channel-adapter> (integration.xml:7)"),
            (16, "'orders' is consumed by: IntegrationFlow.from (FlowConfig.java:13)"),
            (18, "'orders' is consumed by: IntegrationFlow.from (FlowConfig.java:13)"),
        ]);
        assert_eq!(markers[1].target.as_ref().unwrap().path, XML_PATH);
    }

    #[test]
    fn warns_about_lost_messages_and_unknown_beans() {
        let host = setup();
        let mut p = Integration::default();
        p.line_markers(&doc(BILLING_PATH, ENDPOINTS_SRC));
        p.line_markers(&doc(XML_PATH, XML));
        let diagnostics = host.diagnostics();
        let codes = |path: &str| -> Vec<(u32, String)> {
            diagnostics[path].iter().map(|d| (d.range.start.line, d.code.clone().unwrap())).collect()
        };
        assert_eq!(codes(BILLING_PATH), [(9, "unconsumed-channel".into())]);
        assert_eq!(codes(XML_PATH), [(5, "unconsumed-channel".into()), (6, "unknown-bean".into()), (7, "unconsumed-channel".into())]);
        assert!(diagnostics[XML_PATH][1].message.contains("'mailer'"));
    }

    #[test]
    fn completes_channel_and_bean_names() {
        let _host = setup();
        let mut p = Integration::default();
        let text = XML.replace("output-channel=\"lost\"", "output-channel=\"\"");
        let caret = openidea_plugin::text::position(&text, text.find("output-channel=\"\"").unwrap() + 16);
        let items = p.completion(&doc(XML_PATH, &text), caret, None);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"shipping") && labels.contains(&"invoices") && labels.contains(&"late"));
        assert!(!labels.contains(&"billing") && !labels.iter().any(|l| l.contains('$')));

        let items = p.completion(&doc(XML_PATH, XML), at(XML, "\"billing\""), None);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"billing") && labels.contains(&"pricing") && labels.contains(&"flowConfig"));
        assert!(!labels.contains(&"ignored"));
    }

    #[test]
    fn ignores_files_without_integration() {
        let _host = setup();
        let mut p = Integration::default();
        let plain = "<beans xmlns=\"http://www.springframework.org/schema/beans\"><bean id=\"a\"/></beans>";
        assert!(p.line_markers(&doc("/project/src/main/resources/plain.xml", plain)).is_empty());
        assert!(p.completion(&doc("/project/src/main/resources/plain.xml", plain), Position::new(0, 70), None).is_empty());
    }
}
