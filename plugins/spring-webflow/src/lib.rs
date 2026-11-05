//! Spring Web Flow Tools for OpenIDEA: navigation, completion and checks for
//! Spring Web Flow 2/3 XML flow definitions (states, transitions, subflows,
//! parent flows), and a tool window drawing the open flow. Text-based: ids
//! match as written.

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::views::{Body, Button, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Diagnostic, Error, Event, EventKind, Invocation, Location, Plugin, Position, Range, Severity, host, register_plugin,
};
use spring_text::diagram::{self, Edge, Node};
use spring_text::index::{Index, Indexes, PROVIDER_BUDGET};
use spring_text::{beans, contains, range, xml};
use std::collections::{BTreeMap, BTreeSet};

const NAMESPACE: &str = "http://www.springframework.org/schema/webflow";
const CONFIG_NAMESPACE: &str = "http://www.springframework.org/schema/webflow-config";
const STATES: [&str; 5] = ["view-state", "action-state", "decision-state", "subflow-state", "end-state"];
const GLOBS: &[&str] = &["**/*.xml"];
const VIEW: &str = "flow";
const NO_FLOW: &str = "Open a Web Flow definition (flow XML) to see its states.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    /// `to`, `then`, `else`: a state of the flow or its parents.
    State,
    /// `start-state` of `<flow>`.
    Start,
    /// `subflow` of `<subflow-state>`: a flow id.
    Subflow,
    /// `parent` of `<flow>`, or the part before `#` of a state's `parent`.
    ParentFlow,
    /// The part after `#` of a state's `parent`.
    ParentState,
}

/// A reference written in a flow attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub value: String,
    pub kind: LinkKind,
    pub range: Range,
    /// The flow named before `#`, for [`LinkKind::ParentState`].
    pub flow: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub id: String,
    /// The element name: `view-state`, `end-state`…
    pub kind: String,
    pub range: Range,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transition {
    /// `None` for `<global-transitions>`.
    pub from: Option<String>,
    pub to: String,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Flow {
    /// The `flow` name of the root tag.
    pub root: Range,
    pub start: Option<String>,
    pub is_abstract: bool,
    pub parents: Vec<String>,
    pub states: Vec<State>,
    pub transitions: Vec<Transition>,
    pub links: Vec<Link>,
}

impl Flow {
    /// `start-state`, else the first state.
    pub fn start_state(&self) -> Option<&str> {
        self.start.as_deref().or(self.states.first().map(|s| s.id.as_str()))
    }
}

/// What a flow registry (`webflow-config` namespace) says about flow ids.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registry {
    /// `<flow-location id path>`.
    pub locations: Vec<(String, String)>,
    /// `<flow-registry base-path>`.
    pub base_paths: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileInfo {
    pub flow: Option<Flow>,
    pub registry: Registry,
}

/// The parts of an attribute value split at `sep`, trimmed: `(text, from, to)` bytes.
fn pieces(a: &xml::Attr, sep: char) -> Vec<(&str, usize, usize)> {
    let mut at = a.value_at;
    a.value
        .split(sep)
        .map(|part| {
            let lead = part.len() - part.trim_start().len();
            let piece = (part.trim(), at + lead, at + lead + part.trim().len());
            at += part.len() + sep.len_utf8();
            piece
        })
        .collect()
}

/// The id of the state holding `els[i]`.
fn owner(els: &[xml::Element], i: usize) -> Option<String> {
    let mut at = els[i].parent;
    while let Some(p) = at {
        if els[p].parent == Some(0) {
            return els[p].value("id").filter(|_| STATES.contains(&els[p].local())).map(|v| v.trim().to_owned());
        }
        at = els[p].parent;
    }
    None
}

/// The flow of an XML file whose root is a Web Flow `<flow>`.
pub fn parse_flow(text: &str) -> Option<Flow> {
    if !text.contains(NAMESPACE) {
        return None;
    }
    let els = xml::elements(text);
    let root = els.first().filter(|e| e.local() == "flow" && xml::namespace(&els, 0) == Some(NAMESPACE))?;
    let link = |a: &xml::Attr, kind| Link {
        value: a.value.trim().to_owned(),
        kind,
        range: range(text, a.value_at, a.value_end),
        flow: None,
    };
    let piece = |(value, from, to): (&str, usize, usize), kind, flow: Option<&str>| Link {
        value: value.to_owned(),
        kind,
        range: range(text, from, to),
        flow: flow.map(str::to_owned),
    };
    let mut flow = Flow {
        root: range(text, root.at, root.at + root.name.len()),
        is_abstract: root.value("abstract") == Some("true"),
        ..Flow::default()
    };
    if let Some(a) = root.attr("start-state") {
        flow.start = Some(a.value.trim().to_owned());
        flow.links.push(link(a, LinkKind::Start));
    }
    if let Some(a) = root.attr("parent") {
        for p in pieces(a, ',').into_iter().filter(|p| !p.0.is_empty()) {
            flow.parents.push(p.0.to_owned());
            flow.links.push(piece(p, LinkKind::ParentFlow, None));
        }
    }
    for (i, e) in els.iter().enumerate().skip(1) {
        if xml::namespace(&els, i) != Some(NAMESPACE) {
            continue;
        }
        let local = e.local();
        if e.parent == Some(0) && STATES.contains(&local) {
            let Some(id) = e.attr("id") else { continue };
            flow.states.push(State {
                id: id.value.trim().to_owned(),
                kind: local.to_owned(),
                range: range(text, id.value_at, id.value_end),
            });
            if let Some(a) = e.attr("subflow").filter(|_| local == "subflow-state") {
                flow.links.push(link(a, LinkKind::Subflow));
            }
            if let Some(a) = e.attr("parent") {
                match pieces(a, '#')[..] {
                    [f, s] => {
                        flow.links.push(piece(f, LinkKind::ParentFlow, None));
                        flow.links.push(piece(s, LinkKind::ParentState, Some(f.0)));
                    }
                    _ => flow.links.push(link(a, LinkKind::ParentFlow)),
                }
            }
            continue;
        }
        let attrs: &[&str] = match local {
            "transition" => &["to"],
            "if" => &["then", "else"],
            _ => continue,
        };
        let from = owner(&els, i);
        for name in attrs {
            let Some(a) = e.attr(name) else { continue };
            flow.links.push(link(a, LinkKind::State));
            let label = match (local, *name) {
                ("if", "then") => Some(e.value("test").map_or("then".to_owned(), |t| t.trim().to_owned())),
                ("if", _) => Some("else".to_owned()),
                _ => e.value("on").map(str::to_owned),
            };
            flow.transitions.push(Transition { from: from.clone(), to: a.value.trim().to_owned(), label });
        }
    }
    Some(flow)
}

/// The flow locations and base paths of a flow registry.
pub fn parse_registry(text: &str) -> Registry {
    let mut out = Registry::default();
    if !text.contains(CONFIG_NAMESPACE) {
        return out;
    }
    let els = xml::elements(text);
    for (_, e) in els.iter().enumerate().filter(|(i, _)| xml::namespace(&els, *i) == Some(CONFIG_NAMESPACE)) {
        match e.local() {
            "flow-location" => {
                if let (Some(id), Some(path)) = (e.value("id"), e.value("path")) {
                    out.locations.push((id.trim().to_owned(), path.trim().to_owned()));
                }
            }
            "flow-registry" => out.base_paths.extend(e.value("base-path").map(|b| b.trim().to_owned())),
            _ => {}
        }
    }
    out
}

fn index_file(path: &str, text: &str) -> Option<FileInfo> {
    if beans::ignored(path) {
        return None;
    }
    let info = FileInfo { flow: parse_flow(text), registry: parse_registry(text) };
    Some(info).filter(|i| i.flow.is_some() || i.registry != Registry::default())
}

/// The file name without `.xml`: a flow's default id.
fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".xml").unwrap_or(name)
}

/// A registry location as a path tail: `classpath:/flows/a.xml` → `flows/a.xml`.
fn normalize(location: &str) -> &str {
    let location = ["classpath*:", "classpath:", "file:"].iter().fold(location, |l, p| l.strip_prefix(p).unwrap_or(l));
    location.trim_matches('/')
}

/// Every flow id of the project with its file: the file name, then what a
/// flow registry gives it (`flow-location id`, the folder under `base-path`).
fn flow_ids(index: &Index<FileInfo>) -> Vec<(String, &str)> {
    let flows: Vec<&str> = index.iter().filter(|(_, f)| f.flow.is_some()).map(|(p, _)| p).collect();
    let mut out: Vec<(String, &str)> = flows.iter().map(|p| (stem(p).to_owned(), *p)).collect();
    for (_, info) in index.iter() {
        for (id, location) in &info.registry.locations {
            let tail = format!("/{}", normalize(location));
            out.extend(flows.iter().filter(|p| p.ends_with(&tail)).map(|p| (id.clone(), *p)));
        }
        for base in &info.registry.base_paths {
            let dir = format!("/{}/", normalize(base));
            for p in &flows {
                let Some(at) = p.find(&dir) else { continue };
                if let Some((folder, _)) = p[at + dir.len()..].rsplit_once('/') {
                    out.push((folder.to_owned(), p));
                }
            }
        }
    }
    out
}

/// The project's flows, by id.
struct World<'a> {
    index: Option<&'a Index<FileInfo>>,
    ids: Vec<(String, &'a str)>,
}

impl<'a> World<'a> {
    fn new(index: Option<&'a Index<FileInfo>>) -> Self {
        Self { index, ids: index.map(flow_ids).unwrap_or_default() }
    }

    fn complete(&self) -> bool {
        self.index.is_some_and(Index::complete)
    }

    fn flows(&self, id: &str) -> Vec<(&'a str, &'a Flow)> {
        let Some(index) = self.index else { return Vec::new() };
        let paths: BTreeSet<&str> = self.ids.iter().filter(|(i, _)| i == id).map(|(_, p)| *p).collect();
        paths.into_iter().filter_map(|p| Some((p, index.files().get(p)?.flow.as_ref()?))).collect()
    }

    /// The parent flows of `flow` (at `path`), transitively; `false` when one isn't found.
    fn ancestors(&self, path: &str, flow: &Flow) -> (Vec<(&'a str, &'a Flow)>, bool) {
        let mut out = Vec::new();
        let mut found_all = true;
        let mut seen = BTreeSet::from([path.to_owned()]);
        let mut queue = flow.parents.clone();
        while let Some(id) = queue.pop() {
            let found = self.flows(&id);
            found_all &= !found.is_empty();
            for (p, f) in found {
                if seen.insert(p.to_owned()) {
                    queue.extend(f.parents.iter().cloned());
                    out.push((p, f));
                }
            }
        }
        (out, found_all)
    }
}

fn states_named(flows: &[(&str, &Flow)], id: &str) -> Vec<Location> {
    flows
        .iter()
        .flat_map(|(p, f)| f.states.iter().filter(move |s| s.id == id).map(move |s| Location { path: (*p).into(), range: s.range }))
        .collect()
}

fn problems(path: &str, flow: &Flow, world: &World) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for s in &flow.states {
        if !seen.insert(s.id.as_str()) {
            out.push(
                Diagnostic::new(s.range, Severity::Error, format!("Another state of this flow already has the id '{}'.", s.id))
                    .code("duplicate-state"),
            );
        }
    }
    let (parents, found_all) = world.ancestors(path, flow);
    let known: BTreeSet<&str> =
        flow.states.iter().chain(parents.iter().flat_map(|&(_, f)| &f.states)).map(|s| s.id.as_str()).collect();
    for l in flow.links.iter().filter(|l| !l.value.is_empty() && !beans::is_expression(&l.value)) {
        let unknown_state = found_all && !known.contains(l.value.as_str());
        let (severity, message, code) = match l.kind {
            LinkKind::State if unknown_state => (Severity::Error, format!("This flow has no state '{}'.", l.value), "unknown-state"),
            LinkKind::Start if unknown_state => {
                (Severity::Error, format!("The start state '{}' is not a state of this flow.", l.value), "unknown-start-state")
            }
            LinkKind::Subflow if world.complete() && world.flows(&l.value).is_empty() => {
                (Severity::Warning, format!("No flow with the id '{}' is found in the project.", l.value), "unknown-subflow")
            }
            _ => continue,
        };
        out.push(Diagnostic::new(l.range, severity, message).code(code));
    }
    let ends = flow.states.iter().any(|s| s.kind == "end-state");
    if !ends && !flow.is_abstract && flow.parents.is_empty() && !flow.states.is_empty() {
        out.push(
            Diagnostic::new(flow.root, Severity::Info, "This flow has no end-state, so it never finishes.").code("no-end-state"),
        );
    }
    out
}

/// The flow of `doc` and the link under the caret.
fn link_at(doc: &Document, at: Position) -> Option<(Flow, Link)> {
    let flow = parse_flow(&doc.text)?;
    let link = flow.links.iter().find(|l| contains(l.range, at))?.clone();
    Some((flow, link))
}

/// The Html diagram of a flow: states as nodes, transitions as edges.
fn diagram_view(flow: &Flow) -> View {
    let start = flow.start_state();
    let nodes: Vec<Node> =
        flow.states.iter().map(|s| Node::new(&s.id, &s.id).detail(&s.kind).accent(Some(s.id.as_str()) == start)).collect();
    let edges: Vec<Edge> = flow
        .transitions
        .iter()
        .filter_map(|t| {
            let edge = Edge::new(t.from.as_deref()?, &t.to);
            Some(match &t.label {
                Some(l) => edge.label(l),
                None => edge,
            })
        })
        .collect();
    let status = format!("{} states · {} transitions", nodes.len(), edges.len());
    View::new(Body::Html(diagram::html(&nodes, &edges, "This flow has no states."))).status(&status)
}

pub struct WebFlow {
    indexes: Indexes<FileInfo>,
}

impl Default for WebFlow {
    fn default() -> Self {
        Self { indexes: Indexes::new(GLOBS, index_file) }
    }
}

impl WebFlow {
    fn index(&mut self, doc: &Document) -> Option<&Index<FileInfo>> {
        self.indexes.for_document(&doc.path, &doc.text, PROVIDER_BUDGET).map(|i| &*i)
    }
}

impl Providers for WebFlow {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some((flow, link)) = link_at(doc, at) else { return Vec::new() };
        let world = World::new(self.index(doc));
        let (parents, _) = world.ancestors(&doc.path, &flow);
        let items: BTreeMap<&str, &str> = match link.kind {
            LinkKind::State | LinkKind::Start => flow
                .states
                .iter()
                .chain(parents.iter().flat_map(|&(_, f)| &f.states))
                .map(|s| (s.id.as_str(), s.kind.as_str()))
                .collect(),
            LinkKind::Subflow | LinkKind::ParentFlow => world.ids.iter().map(|(id, _)| (id.as_str(), "flow")).collect(),
            LinkKind::ParentState => world
                .flows(link.flow.as_deref().unwrap_or_default())
                .into_iter()
                .flat_map(|(_, f)| &f.states)
                .map(|s| (s.id.as_str(), s.kind.as_str()))
                .collect(),
        };
        items
            .into_iter()
            .filter(|(label, _)| !label.is_empty())
            .map(|(label, detail)| CompletionItem {
                detail: Some(detail.into()),
                replace: Some(link.range),
                ..CompletionItem::new(label, CompletionKind::Reference)
            })
            .collect()
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let Some((flow, link)) = link_at(doc, at) else { return Vec::new() };
        let world = World::new(self.index(doc));
        match link.kind {
            LinkKind::State | LinkKind::Start => {
                let own = states_named(&[(&doc.path, &flow)], &link.value);
                if !own.is_empty() {
                    return own;
                }
                states_named(&world.ancestors(&doc.path, &flow).0, &link.value)
            }
            LinkKind::Subflow | LinkKind::ParentFlow => {
                world.flows(&link.value).into_iter().map(|(p, f)| Location { path: p.into(), range: f.root }).collect()
            }
            LinkKind::ParentState => states_named(&world.flows(link.flow.as_deref().unwrap_or_default()), &link.value),
        }
    }

    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let Some(flow) = parse_flow(&doc.text) else { return Vec::new() };
        let world = World::new(self.index(doc));
        let _ = host::publish_diagnostics(&doc.path, &problems(&doc.path, &flow, &world));
        host::refresh_view(VIEW);
        Vec::new()
    }
}

impl Views for WebFlow {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        let _ = host::subscribe(EventKind::EditorFocused);
        let refresh = Button::new("refresh", "Refresh").icon("refresh");
        let flow = ctx.editor().and_then(|e| e.document().ok()).and_then(|d| parse_flow(&d.text));
        let Some(flow) = flow else { return Ok(View::message(NO_FLOW).button(refresh)) };
        Ok(diagram_view(&flow).button(refresh))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, _ctx: &Invocation) -> Result<(), Error> {
        if matches!(&event, ViewEvent::Button(b) if b == "refresh") {
            host::refresh_view(view);
        }
        Ok(())
    }
}

impl Plugin for WebFlow {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        if events.iter().any(|e| matches!(e, Event::EditorFocused(_))) {
            host::refresh_view(VIEW);
        }
    }
}

register_plugin!(WebFlow, providers, views);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::text::position;

    const FLOW: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<flow xmlns="http://www.springframework.org/schema/webflow"
      xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
      xsi:schemaLocation="http://www.springframework.org/schema/webflow https://www.springframework.org/schema/webflow/spring-webflow.xsd"
      start-state="enterDetails" parent="common">
    <view-state id="enterDetails">
        <transition on="proceed" to="checkAvailability"/>
        <transition on="cancel" to="cancelled"/>
    </view-state>
    <decision-state id="checkAvailability">
        <if test="bookingService.isAvailable(booking)" then="payment" else="soldOut"/>
    </decision-state>
    <subflow-state id="payment" subflow="payment">
        <transition on="paid" to="confirmed"/>
        <transition on="failed" to="enterDetails"/>
    </subflow-state>
    <action-state id="soldOut" parent="common#notify">
        <evaluate expression="bookingService.notifySoldOut(booking)"/>
        <transition to="missing"/>
    </action-state>
    <subflow-state id="audit" subflow="nowhere"/>
    <end-state id="confirmed"/>
    <end-state id="confirmed"/>
    <global-transitions>
        <transition on="error" to="errorPage"/>
    </global-transitions>
</flow>
"#;
    const COMMON: &str = r#"<flow xmlns="http://www.springframework.org/schema/webflow" abstract="true">
    <action-state id="notify">
        <transition to="errorPage"/>
    </action-state>
    <end-state id="cancelled"/>
    <end-state id="errorPage"/>
</flow>
"#;
    const CHECKOUT: &str = r#"<flow xmlns="http://www.springframework.org/schema/webflow">
    <view-state id="pay">
        <transition on="submit" to="done"/>
    </view-state>
    <end-state id="done"/>
</flow>
"#;
    const CONFIG: &str = r#"<beans xmlns="http://www.springframework.org/schema/beans"
       xmlns:webflow="http://www.springframework.org/schema/webflow-config">
    <webflow:flow-registry id="flowRegistry" base-path="/WEB-INF/flows">
        <webflow:flow-location id="payment" path="/pay/checkout.xml"/>
        <webflow:flow-location-pattern value="/**/*-flow.xml"/>
    </webflow:flow-registry>
</beans>
"#;
    const FLOWS: &str = "/project/src/main/webapp/WEB-INF/flows";
    const FLOW_PATH: &str = "/project/src/main/webapp/WEB-INF/flows/booking/booking-flow.xml";

    fn setup() -> MockHost {
        let host = MockHost::install();
        host.add_file("src/main/webapp/WEB-INF/flows/booking/booking-flow.xml", FLOW);
        host.add_file("src/main/webapp/WEB-INF/flows/common.xml", COMMON);
        host.add_file("src/main/webapp/WEB-INF/flows/pay/checkout.xml", CHECKOUT);
        host.add_file("src/main/webapp/WEB-INF/webflow-config.xml", CONFIG);
        host
    }

    fn doc(path: &str, text: &str) -> Document {
        Document { path: path.into(), language: "xml".into(), version: 1, text: text.into() }
    }

    /// The caret at the end of the value of `needle` (`attr="value"`).
    fn at(text: &str, needle: &str) -> Position {
        position(text, text.find(needle).unwrap() + needle.len() - 1)
    }

    fn line(text: &str, needle: &str) -> u32 {
        position(text, text.find(needle).unwrap()).line
    }

    fn found(locations: Vec<Location>) -> Vec<(String, u32)> {
        locations.into_iter().map(|l| (l.path.rsplit('/').next().unwrap().to_owned(), l.range.start.line)).collect()
    }

    fn labels(items: Vec<CompletionItem>) -> Vec<String> {
        items.into_iter().map(|i| i.label).collect()
    }

    #[test]
    fn reads_states_transitions_and_links() {
        let flow = parse_flow(FLOW).unwrap();
        let states: Vec<_> = flow.states.iter().map(|s| (s.id.as_str(), s.kind.as_str())).collect();
        assert_eq!(states, [
            ("enterDetails", "view-state"),
            ("checkAvailability", "decision-state"),
            ("payment", "subflow-state"),
            ("soldOut", "action-state"),
            ("audit", "subflow-state"),
            ("confirmed", "end-state"),
            ("confirmed", "end-state"),
        ]);
        assert_eq!((flow.start_state(), flow.parents.as_slice()), (Some("enterDetails"), &["common".to_owned()][..]));
        let edges: Vec<_> = flow.transitions.iter().map(|t| (t.from.as_deref(), t.to.as_str(), t.label.as_deref())).collect();
        assert_eq!(edges[2..4], [
            (Some("checkAvailability"), "payment", Some("bookingService.isAvailable(booking)")),
            (Some("checkAvailability"), "soldOut", Some("else")),
        ]);
        assert_eq!(edges[6], (Some("soldOut"), "missing", None));
        assert_eq!(edges[7], (None, "errorPage", Some("error")));
        let parent: Vec<_> = flow.links.iter().filter(|l| l.flow.is_some()).map(|l| (l.value.as_str(), l.kind)).collect();
        assert_eq!(parent, [("notify", LinkKind::ParentState)]);

        assert!(parse_flow(CONFIG).is_none());
        assert!(parse_flow("<beans xmlns=\"http://www.springframework.org/schema/webflow\"><flow/></beans>").is_none());
        assert_eq!(parse_registry(CONFIG).locations, [("payment".to_owned(), "/pay/checkout.xml".to_owned())]);
    }

    #[test]
    fn navigates_states_in_the_flow() {
        let _host = setup();
        let mut w = WebFlow::default();
        let d = doc(FLOW_PATH, FLOW);
        let here = |needle| [("booking-flow.xml".to_owned(), line(FLOW, needle))];
        assert_eq!(found(w.definition(&d, at(FLOW, "to=\"checkAvailability\""))), here("id=\"checkAvailability\""));
        assert_eq!(found(w.definition(&d, at(FLOW, "then=\"payment\""))), here("id=\"payment\""));
        assert_eq!(found(w.definition(&d, at(FLOW, "else=\"soldOut\""))), here("id=\"soldOut\""));
        assert_eq!(found(w.definition(&d, at(FLOW, "start-state=\"enterDetails\""))), here("id=\"enterDetails\""));
    }

    #[test]
    fn navigates_to_parent_flows_and_their_states() {
        let _host = setup();
        let mut w = WebFlow::default();
        let d = doc(FLOW_PATH, FLOW);
        let common = |needle| [("common.xml".to_owned(), line(COMMON, needle))];
        assert_eq!(found(w.definition(&d, at(FLOW, "to=\"cancelled\""))), common("id=\"cancelled\""));
        assert_eq!(found(w.definition(&d, at(FLOW, "parent=\"common\""))), [("common.xml".to_owned(), 0)]);
        assert_eq!(found(w.definition(&d, at(FLOW, "parent=\"common#notify\""))), common("id=\"notify\""));
        assert_eq!(found(w.definition(&d, at(FLOW, "parent=\"common#"))), [("common.xml".to_owned(), 0)]);
    }

    #[test]
    fn navigates_to_subflows_by_file_name_and_registry_id() {
        let _host = setup();
        let mut w = WebFlow::default();
        let d = doc(FLOW_PATH, FLOW);
        assert_eq!(found(w.definition(&d, at(FLOW, "subflow=\"payment\""))), [("checkout.xml".to_owned(), 0)]);
        let caller = format!("<flow xmlns=\"{NAMESPACE}\"><subflow-state id=\"s\" subflow=\"common\"/><subflow-state id=\"t\" subflow=\"booking\"/></flow>");
        let d = doc(&format!("{FLOWS}/caller.xml"), &caller);
        assert_eq!(found(w.definition(&d, at(&caller, "subflow=\"common\""))), [("common.xml".to_owned(), 0)]);
        assert_eq!(found(w.definition(&d, at(&caller, "subflow=\"booking\""))), [("booking-flow.xml".to_owned(), 1)]);
    }

    #[test]
    fn completes_states_and_flow_ids() {
        let _host = setup();
        let mut w = WebFlow::default();
        let d = doc(FLOW_PATH, FLOW);
        assert_eq!(labels(w.completion(&d, at(FLOW, "to=\"missing\""), None)), [
            "audit", "cancelled", "checkAvailability", "confirmed", "enterDetails", "errorPage", "notify", "payment", "soldOut",
        ]);
        let flows = ["booking", "booking-flow", "caller", "checkout", "common", "pay", "payment"].map(String::from);
        let caller = format!("<flow xmlns=\"{NAMESPACE}\"/>");
        w.line_markers(&doc(&format!("{FLOWS}/caller.xml"), &caller));
        assert_eq!(labels(w.completion(&d, at(FLOW, "subflow=\"nowhere\""), None)), flows);
        assert_eq!(labels(w.completion(&d, at(FLOW, "parent=\"common\""), None)), flows);
        assert_eq!(labels(w.completion(&d, at(FLOW, "parent=\"common#notify\""), None)), ["cancelled", "errorPage", "notify"]);
        let item = &w.completion(&d, at(FLOW, "then=\"payment\""), None)[0];
        assert_eq!(item.detail.as_deref(), Some("subflow-state"));
    }

    #[test]
    fn reports_unknown_and_duplicate_states_and_subflows() {
        let host = setup();
        let mut w = WebFlow::default();
        w.line_markers(&doc(FLOW_PATH, FLOW));
        let problems: Vec<_> =
            host.diagnostics()[FLOW_PATH].iter().map(|d| (d.range.start.line, d.code.clone().unwrap(), d.severity)).collect();
        assert_eq!(problems, [
            (position(FLOW, FLOW.rfind("id=\"confirmed\"").unwrap()).line, "duplicate-state".into(), Severity::Error),
            (line(FLOW, "to=\"missing\""), "unknown-state".into(), Severity::Error),
            (line(FLOW, "subflow=\"nowhere\""), "unknown-subflow".into(), Severity::Warning),
        ]);
        let messages: Vec<_> = host.diagnostics()[FLOW_PATH].iter().map(|d| d.message.clone()).collect();
        assert_eq!(messages, [
            "Another state of this flow already has the id 'confirmed'.",
            "This flow has no state 'missing'.",
            "No flow with the id 'nowhere' is found in the project.",
        ]);
    }

    #[test]
    fn reports_a_missing_start_state_and_end_state() {
        let host = setup();
        let mut w = WebFlow::default();
        let text = format!("<flow xmlns=\"{NAMESPACE}\" start-state=\"begin\">\n<view-state id=\"a\"><transition on=\"x\" to=\"a\"/></view-state>\n</flow>\n");
        let path = format!("{FLOWS}/loop.xml");
        w.line_markers(&doc(&path, &text));
        let codes: Vec<_> = host.diagnostics()[&path].iter().map(|d| (d.code.clone().unwrap(), d.severity)).collect();
        assert_eq!(codes, [("unknown-start-state".into(), Severity::Error), ("no-end-state".into(), Severity::Info)]);

        let orphan = format!("<flow xmlns=\"{NAMESPACE}\" parent=\"ghost\"><view-state id=\"a\"><transition to=\"b\"/></view-state></flow>");
        let path = format!("{FLOWS}/orphan.xml");
        w.line_markers(&doc(&path, &orphan));
        assert!(!host.diagnostics().contains_key(&path), "states may come from a parent that isn't found");
    }

    #[test]
    fn draws_the_active_flow() {
        let host = setup();
        let mut w = WebFlow::default();
        host.open_editor(FLOW_PATH, "xml", FLOW, &[]);
        let view = w.render(VIEW, None, &host.invocation()).unwrap();
        let Body::Html(page) = view.body else { panic!("html expected") };
        assert!(page.contains(">enterDetails<") && page.contains(">soldOut<") && page.contains("view-state"), "{page}");
        assert!(page.contains("class=\"n a\" data-id=\"enterDetails\""), "the start state is accented");
        assert!(page.contains(">proceed<") && page.contains(">else<"));
        assert!(page.contains("<title>bookingService.isAvailable(booking)</title>bookingService.isAvailable(…<"), "decisions show their test");
        assert_eq!(view.status.as_deref(), Some("7 states · 7 transitions"));
        assert!(host.subscriptions().contains(&EventKind::EditorFocused));

        host.open_editor("/project/pom.xml", "xml", "<project/>", &[]);
        let view = w.render(VIEW, None, &host.invocation()).unwrap();
        assert_eq!(view.body, Body::Message(NO_FLOW.into()));

        w.handle(VIEW, ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
        w.handle_events(&[Event::EditorFocused(Some(FLOW_PATH.into()))], &host.invocation());
        assert_eq!(host.refreshed_views(), [VIEW, VIEW]);
    }
}
