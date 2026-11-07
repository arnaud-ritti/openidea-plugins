//! Spring Messaging Tools for OpenIDEA: links the consumers, producers and
//! declarations of Kafka topics, RabbitMQ queues and exchanges and JMS
//! destinations in a Java project by destination name. Text-based.

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Error, Invocation, Location, Plugin, Position, Range, host};
use spring_text::index::{COMMAND_BUDGET, Indexes, PROVIDER_BUDGET, project_of};
use openidea_plugin::Project;
use spring_text::constants::Constants;
use spring_text::{config, xml};
use spring_text::java::{self, Annotation, Arg, Call, CallArg, JavaFile, Str, Tok};
use spring_text::{parse_source, range, source_calls};
use spring_text::tree::Expansion;
use std::collections::{BTreeMap, BTreeSet, HashMap};

mod listener_xml;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Broker {
    Kafka,
    Rabbit,
    Jms,
}

impl Broker {
    fn label(self) -> &'static str {
        match self {
            Broker::Kafka => "Kafka",
            Broker::Rabbit => "RabbitMQ",
            Broker::Jms => "JMS",
        }
    }

    /// The broker a template's name or type is for (`kafkaTemplate`, `RabbitTemplate`).
    fn hinted_by(name: &str) -> Option<Broker> {
        let name = name.to_lowercase();
        if name.contains("kafka") {
            Some(Broker::Kafka)
        } else if name.contains("rabbit") || name.contains("amqp") {
            Some(Broker::Rabbit)
        } else if name.contains("jms") {
            Some(Broker::Jms)
        } else {
            None
        }
    }

    fn of_listener(annotations: &[Annotation]) -> Option<Broker> {
        annotations.iter().find_map(|a| match a.name.as_str() {
            "KafkaListener" => Some(Broker::Kafka),
            "RabbitListener" => Some(Broker::Rabbit),
            "JmsListener" => Some(Broker::Jms),
            _ => None,
        })
    }
}

/// Listed in this order: what is consumed first, then who sends, then where it's declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Consumer,
    Producer,
    Declaration,
}

/// A destination named in a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub broker: Broker,
    /// `topic`, `queue`, `exchange` or `destination`: names match within one.
    pub space: &'static str,
    pub name: String,
    pub role: Role,
    /// What names it: `@KafkaListener`, `send`, `new NewTopic`…
    pub via: String,
    /// The bytes and range of the literal's contents.
    pub at: usize,
    pub end: usize,
    pub range: Range,
}

impl Endpoint {
    fn same_destination(&self, o: &Endpoint) -> bool {
        self.broker == o.broker && self.space == o.space && self.name == o.name
    }

    fn holds(&self, at: usize) -> bool {
        self.at <= at && at <= self.end
    }
}

fn space(broker: Broker) -> &'static str {
    match broker {
        Broker::Kafka => "topic",
        Broker::Rabbit => "queue",
        Broker::Jms => "destination",
    }
}

/// The annotations written in an annotation argument (`bindings = @QueueBinding(…)`).
fn nested(text: &str, toks: &[Tok], arg: Option<&Arg>) -> Vec<Annotation> {
    let Some(arg) = arg else { return Vec::new() };
    let inner: Vec<Tok> = toks.iter().filter(|t| t.at >= arg.at && t.end() <= arg.end).copied().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < inner.len() {
        i = if inner[i].s == "@" { java::annotation(text, &inner, i, &mut out) } else { i + 1 };
    }
    out
}

/// A post processor or correlation data after the message (`convertAndSend(key, msg, m -> m)`).
fn is_extra(a: &CallArg) -> bool {
    a.text.contains("->")
        || a.text.contains("::")
        || a.text.to_lowercase().contains("correlation")
        || a.text.contains("PostProcessor")
}

/// What a `RabbitTemplate` send names: `(routingKey, msg)` goes through the
/// default exchange to the queue of that name, `(exchange, routingKey, msg)`
/// to the exchange.
fn rabbit_target(c: &Call) -> Option<(&'static str, Str)> {
    let literal = |i: usize| c.args.get(i).and_then(arg_str);
    let n = c.args.len();
    let first = literal(0).filter(|_| n >= 2)?;
    if n == 2 || (n == 3 && literal(1).is_none() && is_extra(&c.args[2])) {
        return Some(("queue", first));
    }
    if first.value.is_empty() {
        return literal(1).map(|key| ("queue", key));
    }
    Some(("exchange", first))
}

/// Starts the name of a destination written as a constant (`Queues.ORDERS`), until the project's constants resolve it.
const CONSTANT: char = '\u{1}';

/// `Type.NAME` of a constant expression: the last two segments, or `own.NAME` for a bare name.
fn constant_key(expr: &str, own: Option<&str>) -> Option<String> {
    let ident = |p: &str| p.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && p.chars().all(|c| c.is_alphanumeric() || c == '_');
    let parts: Vec<&str> = expr.split('.').collect();
    if !parts.iter().all(|p| ident(p)) {
        return None;
    }
    match parts.as_slice() {
        [name] => Some(format!("{}.{name}", own?)),
        [.., ty, name] => Some(format!("{ty}.{name}")),
        [] => None,
    }
}

fn constant_str(expr: &str, at: usize, own: Option<&str>) -> Option<Str> {
    let key = constant_key(expr, own)?;
    Some(Str { value: format!("{CONSTANT}{key}"), at, end: at + expr.len() })
}

/// The destination a whole-value SpEL template names: `#{'orders'}`, `#{"a" + 'b'}` and `#{T(com.acme.Queues).ORDERS}`.
/// `None` for anything that needs evaluating (method calls, beans, conditions).
fn spel_destination(value: &str) -> Option<String> {
    let body = value.trim().strip_prefix("#{")?.strip_suffix('}')?;
    let mut out = String::new();
    let mut constants = 0;
    for operand in body.split('+').map(str::trim) {
        if constants > 0 {
            return None;
        }
        let quoted = ['\'', '"'].into_iter().find_map(|q| operand.strip_prefix(q)?.strip_suffix(q));
        if let Some(literal) = quoted {
            if literal.contains(['\'', '"', '#']) {
                return None;
            }
            out.push_str(literal);
            continue;
        }
        let reference = operand.strip_prefix("T(")?;
        let (ty, name) = reference.split_once(").")?;
        let ty = ty.rsplit('.').next()?;
        let ident = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || c == '_');
        if !ident(ty) || !ident(name) || constants > 0 || !out.is_empty() {
            return None;
        }
        constants += 1;
        out = format!("{CONSTANT}{ty}.{name}");
    }
    (!out.is_empty()).then_some(out)
}

/// The constants named in an annotation attribute (`queues = {Queues.A, Queues.B}`).
fn constant_strs(arg: &Arg, own: Option<&str>) -> Vec<Str> {
    let mut at = arg.at;
    let mut out = Vec::new();
    for piece in arg.text.split(',') {
        let wrapped = |c: char| c.is_whitespace() || "{}[]".contains(c);
        let lead = piece.len() - piece.trim_start_matches(wrapped).len();
        out.extend(constant_str(piece.trim_matches(wrapped), at + lead, own));
        at += piece.len() + 1;
    }
    out
}

/// The literal of a call argument, or the constant it names (`Topics.ORDERS`).
fn arg_str(a: &CallArg) -> Option<Str> {
    a.string.clone().or_else(|| a.text.contains('.').then(|| constant_str(a.text.trim(), a.at, None)).flatten())
}

struct Collector<'a> {
    text: &'a str,
    out: Vec<Endpoint>,
    own: Option<String>,
}

impl Collector<'_> {
    fn add(&mut self, broker: Broker, space: &'static str, role: Role, via: &str, s: &Str) {
        let v = &s.value;
        if v.is_empty() || v.starts_with("!{") {
            return;
        }
        let name = if v.contains("#{") {
            match spel_destination(v) {
                Some(name) => name,
                None => return,
            }
        } else {
            v.clone()
        };
        let range = range(self.text, s.at, s.end);
        self.out.push(Endpoint {
            broker,
            space,
            name,
            role,
            via: via.to_owned(),
            at: s.at,
            end: s.end,
            range,
        });
    }

    fn listener(&mut self, a: &Annotation) {
        let via = format!("@{}", a.name);
        let (broker, attrs): (Broker, &[&str]) = match a.name.as_str() {
            "KafkaListener" => (Broker::Kafka, &["topics"]),
            "RabbitListener" => (Broker::Rabbit, &["queues"]),
            "JmsListener" => (Broker::Jms, &["destination"]),
            _ => return,
        };
        for s in a.strings(attrs) {
            self.add(broker, space(broker), Role::Consumer, &via, s);
        }
        if let Some(arg) = attrs.iter().find_map(|n| a.arg(n)) {
            for s in constant_strs(arg, self.own.as_deref()) {
                self.add(broker, space(broker), Role::Consumer, &via, &s);
            }
        }
        if broker != Broker::Rabbit || (a.arg("bindings").is_none() && a.arg("queuesToDeclare").is_none()) {
            return;
        }
        let text = self.text;
        let toks = java::tokens(text);
        let name = |a: &Annotation| a.strings(&["value", "name"]).first().map(|s| (*s).clone());
        for q in nested(text, &toks, a.arg("queuesToDeclare")).iter().filter_map(name) {
            self.add(broker, "queue", Role::Consumer, &via, &q);
        }
        for binding in nested(text, &toks, a.arg("bindings")) {
            for q in nested(text, &toks, binding.arg("value")).iter().filter_map(name) {
                self.add(broker, "queue", Role::Consumer, &via, &q);
            }
            for x in nested(text, &toks, binding.arg("exchange")).iter().filter_map(name) {
                self.add(broker, "exchange", Role::Consumer, &via, &x);
            }
        }
    }

    /// `@SendTo` on a listener method: a reply destination of its broker.
    fn reply(&mut self, broker: Broker, s: &Str) {
        if broker == Broker::Rabbit && s.value.contains("${") {
            return;
        }
        if broker != Broker::Rabbit {
            return self.add(broker, space(broker), Role::Producer, "@SendTo", s);
        }
        // `exchange/routingKey`; no exchange: the default one, to the queue named by the key.
        match s.value.split_once('/') {
            Some(("", key)) => {
                let at = s.end - key.len();
                self.add(broker, "queue", Role::Producer, "@SendTo", &Str { value: key.to_owned(), at, end: s.end });
            }
            Some((exchange, _)) => {
                let end = s.at + exchange.len();
                self.add(
                    broker,
                    "exchange",
                    Role::Producer,
                    "@SendTo",
                    &Str { value: exchange.to_owned(), at: s.at, end },
                );
            }
            None => self.add(broker, "queue", Role::Producer, "@SendTo", s),
        }
    }

    fn call(&mut self, c: &Call, file: &JavaFile) {
        let Some(first) = c.args.first().and_then(arg_str) else { return };
        if c.is_new {
            let (broker, space, role) = match c.name.as_str() {
                "NewTopic" => (Broker::Kafka, "topic", Role::Declaration),
                "ProducerRecord" => (Broker::Kafka, "topic", Role::Producer),
                "Queue" => (Broker::Rabbit, "queue", Role::Declaration),
                "TopicExchange" | "DirectExchange" | "FanoutExchange" | "HeadersExchange" => {
                    (Broker::Rabbit, "exchange", Role::Declaration)
                }
                "ActiveMQQueue" | "ActiveMQTopic" => (Broker::Jms, "destination", Role::Declaration),
                _ => return,
            };
            return self.add(broker, space, role, &format!("new {}", c.name), &first);
        }
        let Some(receiver) = c.receiver.as_deref() else { return };
        let via = format!("{receiver}.{}", c.name);
        match (receiver, c.name.as_str()) {
            ("TopicBuilder", "name") => self.add(Broker::Kafka, "topic", Role::Declaration, &via, &first),
            ("QueueBuilder", "durable" | "nonDurable") => {
                self.add(Broker::Rabbit, "queue", Role::Declaration, &via, &first)
            }
            ("ExchangeBuilder", "topicExchange" | "directExchange" | "fanoutExchange" | "headersExchange") => {
                self.add(Broker::Rabbit, "exchange", Role::Declaration, &via, &first)
            }
            (_, "send" | "convertAndSend" | "convertSendAndReceive" | "sendAndReceive") => {
                match Broker::hinted_by(receiver).or_else(|| declared_type(file, receiver).and_then(Broker::hinted_by))
                {
                    Some(Broker::Kafka) if c.name == "send" => {
                        self.add(Broker::Kafka, "topic", Role::Producer, &c.name, &first)
                    }
                    Some(Broker::Rabbit) => {
                        if let Some((space, s)) = rabbit_target(c) {
                            self.add(Broker::Rabbit, space, Role::Producer, &c.name, &s);
                        }
                    }
                    Some(Broker::Jms) if c.args.len() >= 2 => {
                        self.add(Broker::Jms, "destination", Role::Producer, &c.name, &first)
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

/// The type of a field or parameter named `name` in the file.
fn declared_type<'f>(file: &'f JavaFile, name: &str) -> Option<&'f str> {
    file.types.iter().flat_map(|t| &t.members).find_map(|m| {
        if m.name == name {
            return m.ty.as_ref().filter(|_| m.kind == java::MemberKind::Field).map(|t| t.name.as_str());
        }
        m.params.iter().find(|p| p.name == name).map(|p| p.ty.name.as_str())
    })
}

/// `<jms:listener destination>` and `<rabbit:listener queue-names>` of a Spring XML file.
fn xml_listeners(text: &str, collector: &mut Collector) {
    let elements = xml::elements(text);
    for (i, e) in elements.iter().enumerate() {
        if e.local() != "listener" {
            continue;
        }
        let Some(namespace) = xml::namespace(&elements, i) else { continue };
        let (broker, attr) = if namespace.ends_with("/schema/jms") {
            (Broker::Jms, "destination")
        } else if namespace.ends_with("/schema/rabbit") {
            (Broker::Rabbit, "queue-names")
        } else {
            continue;
        };
        let Some(a) = e.attr(attr) else { continue };
        let via = format!("<{}>", e.name);
        let mut at = a.value_at;
        for part in a.value.split(',') {
            let lead = part.len() - part.trim_start().len();
            let value = part.trim().to_owned();
            let (start, end) = (at + lead, at + lead + value.len());
            at += part.len() + 1;
            collector.add(broker, space(broker), Role::Consumer, &via, &Str { value, at: start, end });
        }
    }
}

pub fn parse(path: &str, text: &str) -> Option<Vec<Endpoint>> {
    if path.ends_with(".xml") {
        if !text.contains("listener") {
            return None;
        }
        let mut collector = Collector { text, out: Vec::new(), own: None };
        xml_listeners(text, &mut collector);
        return (!collector.out.is_empty()).then_some(collector.out);
    }
    const HINTS: [&str; 7] = ["Listener", "send", "Send", "Topic", "Queue", "Exchange", "ProducerRecord"];
    if !HINTS.iter().any(|h| text.contains(h)) {
        return None;
    }
    let file = parse_source(path, text);
    let mut collector = Collector { text, out: Vec::new(), own: None };
    for t in &file.types {
        collector.own = Some(t.name.clone());
        for a in &t.annotations {
            collector.listener(a);
        }
        let class_broker = Broker::of_listener(&t.annotations);
        for m in t.methods() {
            for a in &m.annotations {
                collector.listener(a);
            }
            if let Some(broker) = Broker::of_listener(&m.annotations).or(class_broker) {
                for s in m.annotation("SendTo").map(|a| a.strings(&["value"])).unwrap_or_default() {
                    collector.reply(broker, s);
                }
            }
        }
    }
    for c in source_calls(path, text) {
        collector.call(&c, &file);
    }
    let mut out = collector.out;
    out.sort_by_key(|e| e.at);
    (!out.is_empty()).then_some(out)
}

/// The string constants a source declares, as `Type.NAME` and value.
fn parse_constants(path: &str, text: &str) -> Option<Vec<(String, String)>> {
    if !text.contains("static final") && !text.contains("const val") {
        return None;
    }
    let file = parse_source(path, text);
    let mut constants = Constants::default();
    constants.add(&file, text);
    let found: Vec<_> = file
        .types
        .iter()
        .flat_map(|t| t.fields().map(move |f| format!("{}.{}", t.name, f.name)))
        .filter_map(|key| constants.resolve(&key, None).map(|value| (key, value)))
        .collect();
    (!found.is_empty()).then_some(found)
}

/// Configuration values by relaxed key: `application.*` files win over profile and bootstrap ones.
type Props = BTreeMap<String, String>;

fn parse_config(path: &str, text: &str) -> Option<Vec<(String, String)>> {
    let keys: Vec<_> = config::keys(path, text)
        .into_iter()
        .filter(|k| !k.value.is_empty())
        .map(|k| (config::canonical(&k.key), k.value))
        .collect();
    (!keys.is_empty()).then_some(keys)
}

fn props_of<'a>(files: impl Iterator<Item = (&'a str, &'a Vec<(String, String)>)>) -> Props {
    let (mut primary, mut others) = (Vec::new(), Vec::new());
    for (path, keys) in files {
        let stem = file_name(path).split('.').next().unwrap_or_default();
        if stem == "application" { primary.push(keys) } else { others.push(keys) }
    }
    let mut props = Props::new();
    for (key, value) in primary.into_iter().chain(others).flatten() {
        props.entry(key.clone()).or_insert_with(|| value.clone());
    }
    props
}

/// Replaces every `${key}` and `${key:default}`, a value or default holding
/// placeholders itself down to `depth` levels. `None` when one can't be told.
fn expand(text: &str, props: &Props, depth: u8) -> Option<String> {
    let Some(start) = text.find("${") else { return Some(text.to_owned()) };
    if depth == 0 {
        return None;
    }
    let mut level = 0;
    let mut close = None;
    for (i, c) in text[start..].char_indices() {
        match c {
            '$' if text[start + i..].starts_with("${") => level += 1,
            '}' => {
                level -= 1;
                if level == 0 {
                    close = Some(start + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    let body = &text[start + 2..close];
    let (key, default) = match body.split_once(':') {
        Some((key, default)) if !key.contains("${") => (key, Some(default)),
        _ => (body, None),
    };
    let value = match props.get(&config::canonical(key.trim())) {
        Some(value) => expand(value, props, depth - 1)?,
        None => expand(default?, props, depth - 1)?,
    };
    let rest = expand(&text[close + 1..], props, depth)?;
    Some(format!("{}{value}{rest}", &text[..start]))
}

/// The endpoint with its placeholders resolved; `None` when they can't be, or the name isn't one destination.
fn resolved(mut e: Endpoint, props: &Props, constants: &HashMap<String, String>) -> Option<Endpoint> {
    if let Some(key) = e.name.strip_prefix(CONSTANT) {
        e.name = constants.get(key)?.clone();
        if e.name.is_empty() {
            return None;
        }
    }
    if e.name.contains("${") {
        e.name = expand(&e.name, props, 2)?;
        if e.name.is_empty() || e.name.contains([',', ' ', '$', '#']) {
            return None;
        }
    }
    Some(e)
}

fn location(path: &str, e: &Endpoint) -> Location {
    Location { path: path.to_owned(), range: e.range }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn plural(n: usize, noun: &str) -> String {
    if n == 1 { format!("1 {noun}") } else { format!("{n} {noun}s") }
}

fn title(e: &Endpoint) -> String {
    format!("{} {} {}", e.broker.label(), e.space, e.name)
}

/// (name, space) → the endpoints of that destination with their files.
type Destinations<'a> = BTreeMap<(&'a str, &'static str), Vec<(&'a str, &'a Endpoint)>>;

pub struct Messaging {
    index: Indexes<Vec<Endpoint>>,
    config: Indexes<Vec<(String, String)>>,
    constants: Indexes<Vec<(String, String)>>,
    listener_beans: Indexes<listener_xml::Info>,
    expansion: Expansion,
}

const CONFIG_GLOBS: [&str; 6] = [
    "**/application*.properties",
    "**/application*.yml",
    "**/application*.yaml",
    "**/bootstrap*.properties",
    "**/bootstrap*.yml",
    "**/bootstrap*.yaml",
];

impl Default for Messaging {
    fn default() -> Self {
        Self {
            index: Indexes::new(&["**/*.java", "**/*.kt", "**/*.xml"], parse),
            config: Indexes::new(&CONFIG_GLOBS, parse_config),
            constants: Indexes::new(&["**/*.java", "**/*.kt"], parse_constants),
            listener_beans: Indexes::new(&["**/*.java", "**/*.kt", "**/*.xml"], listener_xml::parse),
            expansion: Expansion::default() }
    }
}

impl Messaging {
    fn constant_values(&mut self, project: &Project, budget: Duration) -> HashMap<String, String> {
        self.constants.get(project, budget).iter().flat_map(|(_, found)| found.iter().cloned()).collect()
    }

    /// Every endpoint of the document's project, by file.
    fn endpoints(&mut self, doc: &Document) -> Vec<(String, Endpoint)> {
        let Some(project) = project_of(&doc.path) else { return Vec::new() };
        let props = props_of(self.config.get(&project, PROVIDER_BUDGET).iter());
        let constants = self.constant_values(&project, PROVIDER_BUDGET);
        let Some(index) = self.index.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else { return Vec::new() };
        index
            .iter()
            .flat_map(|(p, es)| es.iter().filter_map(|e| resolved(e.clone(), &props, &constants)).map(move |e| (p.to_owned(), e)))
            .collect()
    }
}

/// The endpoints of `doc` and the others of the same destination, by role.
fn links<'a>(doc: &Document, all: &'a [(String, Endpoint)]) -> Vec<(&'a Endpoint, Vec<&'a (String, Endpoint)>)> {
    all.iter()
        .filter(|(p, _)| *p == doc.path)
        .map(|(_, e)| {
            let mut others: Vec<_> =
                all.iter().filter(|(p, o)| o.same_destination(e) && !(*p == doc.path && o.at == e.at)).collect();
            others.sort_by_key(|(p, o)| (o.role, p.clone(), o.at));
            (e, others)
        })
        .collect()
}

impl Messaging {
    /// Bean names for a `<rabbit:listener ref>` or `<jms:listener ref>`, the methods of that bean for `method`.
    fn listener_completion(&mut self, doc: &Document, at: usize) -> Vec<CompletionItem> {
        if doc.language != "xml" {
            return Vec::new();
        }
        let Some(beans) = self.listener_beans.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else { return Vec::new() };
        let Some((target, _, from, to)) = listener_xml::target_at(&doc.text, beans, at) else { return Vec::new() };
        let replace = Some(range(&doc.text, from, to));
        match target {
            listener_xml::Target::Bean => listener_xml::bean_names(beans)
                .into_iter()
                .map(|(name, class)| CompletionItem { detail: class, replace, ..CompletionItem::new(&name, CompletionKind::Reference) })
                .collect(),
            listener_xml::Target::Method(Some(class)) => listener_xml::method_names(beans, &class)
                .into_iter()
                .map(|name| CompletionItem { replace, ..CompletionItem::new(&name, CompletionKind::Method) })
                .collect(),
            listener_xml::Target::Method(None) => Vec::new(),
        }
    }
}

impl Providers for Messaging {
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        if doc.language == "xml" {
            let problems = match self.listener_beans.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) {
                Some(beans) => listener_xml::problems(&doc.text, beans),
                None => Vec::new(),
            };
            publish(&doc.path, &problems);
        }
        let all = self.endpoints(doc);
        links(doc, &all)
            .into_iter()
            .filter_map(|(e, others)| {
                let wanted = |o: &Endpoint| match e.role {
                    Role::Consumer => o.role != Role::Consumer,
                    Role::Producer => o.role == Role::Consumer,
                    Role::Declaration => false,
                };
                let others: Vec<_> = others.into_iter().filter(|(_, o)| wanted(o)).collect();
                let (path, first) = others.first()?;
                let list: Vec<String> = others
                    .iter()
                    .map(|(p, o)| format!("{}:{} ({})", file_name(p), o.range.start.line + 1, o.via))
                    .collect();
                Some(LineMarker {
                    line: e.range.start.line,
                    icon: None,
                    tooltip: format!("{}: {}", title(e), list.join(", ")),
                    target: Some(location(path, first)),
                })
            })
            .collect()
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let all = self.endpoints(doc);
        let destinations: Vec<Location> = links(doc, &all)
            .into_iter()
            .filter(|(e, _)| e.holds(at))
            .flat_map(|(_, others)| others.into_iter().map(|(p, o)| location(p, o)))
            .collect();
        if !destinations.is_empty() || doc.language != "xml" {
            return destinations;
        }
        let Some(beans) = self.listener_beans.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else { return Vec::new() };
        match listener_xml::target_at(&doc.text, beans, at) {
            Some((listener_xml::Target::Bean, name, ..)) => listener_xml::bean_locations(beans, &name),
            Some((listener_xml::Target::Method(Some(class)), name, ..)) => listener_xml::method_locations(beans, &class, &name),
            _ => Vec::new(),
        }
    }

    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let all = self.endpoints(doc);
        let Some(here) = all.iter().find(|(p, e)| *p == doc.path && e.holds(at)).map(|(_, e)| e) else {
            return self.listener_completion(doc, at);
        };
        let replace = range(&doc.text, here.at, here.end);
        let mut seen = BTreeSet::new();
        all.iter()
            .filter(|(p, e)| {
                e.broker == here.broker
                    && e.space == here.space
                    && !(*p == doc.path && e.at == here.at)
                    && seen.insert(&e.name)
            })
            .map(|(_, e)| CompletionItem {
                detail: Some(format!("{} {}", e.broker.label(), e.space)),
                replace: Some(replace),
                ..CompletionItem::new(&e.name, CompletionKind::Value)
            })
            .collect()
    }
}

impl Views for Messaging {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        let Some(project) = ctx.project() else {
            return Ok(View::message("Open a project to see its messaging destinations."));
        };
        let props = props_of(self.config.get(&project, COMMAND_BUDGET).iter());
        let constants = self.constant_values(&project, COMMAND_BUDGET);
        let index = self.index.get(&project, COMMAND_BUDGET);
        let complete = index.complete();
        if !complete {
            refresh(view);
        }
        let refresh = Button::new("refresh", "Refresh").icon("refresh");
        let mut tree: BTreeMap<Broker, Destinations> = BTreeMap::new();
        let all: Vec<(&str, Endpoint)> = index
            .iter()
            .flat_map(|(path, es)| es.iter().filter_map(|e| resolved(e.clone(), &props, &constants)).map(move |e| (path, e)))
            .collect();
        for (path, e) in &all {
            tree.entry(e.broker).or_default().entry((e.name.as_str(), e.space)).or_default().push((*path, e));
        }
        if tree.is_empty() {
            let text = if complete { "No Kafka, RabbitMQ or JMS destinations found." } else { "Scanning…" };
            return Ok(View::message(text).button(refresh));
        }
        let mut items = Vec::new();
        let mut total = 0;
        for (broker, destinations) in tree {
            let b = format!("b:{}", broker.label());
            items.push(TreeItem::new(&b, broker.label()).detail(&destinations.len().to_string()).expandable(true));
            total += destinations.len();
            for ((name, space), mut uses) in destinations {
                let d = format!("d:{}:{space}:{name}", broker.label());
                let count = |role| uses.iter().filter(|(_, e)| e.role == role).count();
                let (producers, consumers) = (count(Role::Producer), count(Role::Consumer));
                let detail = format!("{space} · {} · {}", plural(producers, "producer"), plural(consumers, "consumer"));
                let icon = if (producers > 0) != (consumers > 0) { "warning" } else { "send" };
                items.push(TreeItem::new(&d, name).parent(&b).detail(&detail).icon(icon).expandable(false));
                uses.sort_by_key(|(p, e)| (e.role, *p, e.at));
                for (path, e) in uses {
                    let key = format!("e:{}:{}:{path}", e.range.start.line, e.range.start.character);
                    let label = format!("{}:{}", file_name(path), e.range.start.line + 1);
                    let (role, icon) = match e.role {
                        Role::Consumer => ("consumer", "vcs-incoming"),
                        Role::Producer => ("producer", "vcs-outgoing"),
                        Role::Declaration => ("declaration", "spring-config"),
                    };
                    let detail = format!("{role} · {}", e.via);
                    items.push(TreeItem::new(&key, &label).parent(&d).detail(&detail).icon(icon));
                }
            }
        }
        self.expansion.apply(&mut items);
        let mut status = plural(total, "destination");
        if !complete {
            status.push_str(" · scanning…");
        }
        Ok(View::new(Body::Tree(items)).button(refresh).status(&status))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if self.expansion.handle(&event) {
            refresh(view);
            return Ok(());
        }

        match event {
            ViewEvent::Button(b) if b == "refresh" => {
                // Resetting a scan in progress would throw away what it has read.
                if let Some(project) = ctx.project()
                    && self.index.get(&project, Duration::ZERO).complete()
                {
                    self.index.reset();
                    self.config.reset();
                    self.constants.reset();
                    self.listener_beans.reset();
                }
                refresh(view);
            }
            ViewEvent::Activate(key) => {
                let mut parts = key.splitn(4, ':');
                if let (Some("e"), Some(line), Some(col), Some(path)) =
                    (parts.next(), parts.next(), parts.next(), parts.next())
                {
                    host::open_location(path, line.parse().unwrap_or(0), col.parse().unwrap_or(0))?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl Plugin for Messaging {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

pub const WINDOW_SECTION: &str = "messaging";

const OWNER_WINDOW: &str = "endpoints";

fn window(view: &str) -> &str {
    if cfg!(feature = "embedded") { OWNER_WINDOW } else { view }
}

pub(crate) fn refresh(view: &str) {
    host::refresh_view(window(view));
}

thread_local! {
    static PROBLEMS: std::cell::RefCell<Vec<openidea_plugin::Diagnostic>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The diagnostics belong to the owner plugin, which publishes every module's together: a publish replaces the whole file.
fn publish(path: &str, problems: &[openidea_plugin::Diagnostic]) {
    if cfg!(feature = "embedded") {
        PROBLEMS.set(problems.to_vec());
        return;
    }
    let _ = host::publish_diagnostics(path, problems);
}

pub fn take_problems() -> Vec<openidea_plugin::Diagnostic> {
    PROBLEMS.take()
}

const BUILD_FILES: [&str; 3] = ["pom.xml", "build.gradle", "build.gradle.kts"];
const DEPENDENCY_MARKERS: [&str; 12] = ["kafka", "rabbit", "amqp", "jms", "activemq", "artemis", "<module>", "include(", "include '", "include \"", "(libs.", " libs."];

/// True when unsure: a multi-module build or a version catalog hides the dependency from the root build file.
pub fn applies(project: &openidea_plugin::Project) -> bool {
    let Ok(root) = project.root() else { return true };
    BUILD_FILES
        .iter()
        .filter_map(|file| project.read_file(&format!("{root}/{file}")).ok())
        .any(|build| DEPENDENCY_MARKERS.iter().any(|marker| build.contains(marker)))
}

#[cfg(not(feature = "embedded"))]
openidea_plugin::register_plugin!(Messaging, providers, views);

#[cfg(test)]
#[cfg_attr(feature = "embedded", allow(dead_code, unused_imports))]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::Severity;

    const VIEW: &str = "messaging";

    const KAFKA: &str = r#"package com.acme.orders;

@Service
class OrderEvents {
    private final KafkaTemplate<String, Order> template;

    void publish(Order o) {
        template.send("orders", o.id(), o);
        streamBridge.send("orders-out", o);
        kafka.send(new ProducerRecord<>("audit", o));
    }

    @KafkaListener(topics = {"orders", "${extra.topic}"}, groupId = "billing")
    @SendTo("invoices")
    Invoice bill(Order o) { return null; }

    @Bean NewTopic orders() { return TopicBuilder.name("orders").partitions(3).build(); }
}
"#;

    const RABBIT: &str = r#"package com.acme.mail;

@Component
class Mailer {
    private final RabbitTemplate rabbit;

    void send(Mail m) {
        rabbit.convertAndSend("mails", m);
        rabbit.convertAndSend("events", "mail.sent", m);
        rabbit.convertAndSend("", "direct-q", m);
        rabbit.convertAndSend("delayed", m, msg -> msg);
    }

    @RabbitListener(bindings = @QueueBinding(value = @Queue("mail-events"), exchange = @Exchange(value = "events", type = "topic"), key = "mail.*"))
    void onEvent(Event e) {}

    @RabbitListener(queues = "mails")
    @SendTo("replies/done")
    String onMail(Mail m) { return "ok"; }

    @Bean Queue mails() { return QueueBuilder.durable("mails").build(); }
    @Bean TopicExchange events() { return new TopicExchange("events"); }

    @JmsListener(destination = "audit")
    void audit(String s) { jmsTemplate.convertAndSend("audit.out", s); }
}
"#;

    fn names(text: &str) -> Vec<(Broker, &'static str, String, Role, String)> {
        parse("A.java", text)
            .unwrap()
            .into_iter()
            .filter(|e| !e.name.contains("${"))
            .map(|e| (e.broker, e.space, e.name, e.role, e.via))
            .collect()
    }

    const KOTLIN: &str = r#"package com.acme.orders

@Component
class KtEvents(private val kafkaTemplate: KafkaTemplate<String, String>, val rabbit: RabbitTemplate) {
    @KafkaListener(topics = ["orders", "audit"], groupId = "g")
    @SendTo("replies")
    suspend fun onOrder(msg: String): String = msg

    @RabbitListener(queues = [QUEUE])
    fun onMail(msg: String) {}

    @JmsListener(destination = "invoices")
    fun onInvoice(msg: String) = Unit

    fun publish() = kafkaTemplate.send("orders", "x")

    fun mail() { rabbit.convertAndSend("mails", "x") }

    companion object {
        const val QUEUE = "mails"
    }
}
"#;

    #[test]
    fn kotlin_kafka_listener_is_indexed() {
        use Broker::{Jms, Kafka, Rabbit};
        let found: Vec<_> = parse("K.kt", KOTLIN)
            .unwrap()
            .into_iter()
            .map(|e| (e.broker, e.space, e.name, e.role, e.via))
            .collect();
        assert_eq!(found, [
            (Kafka, "topic", "orders".into(), Role::Consumer, "@KafkaListener".into()),
            (Kafka, "topic", "audit".into(), Role::Consumer, "@KafkaListener".into()),
            (Kafka, "topic", "replies".into(), Role::Producer, "@SendTo".into()),
            (Rabbit, "queue", format!("{CONSTANT}KtEvents.QUEUE"), Role::Consumer, "@RabbitListener".into()),
            (Jms, "destination", "invoices".into(), Role::Consumer, "@JmsListener".into()),
            (Kafka, "topic", "orders".into(), Role::Producer, "send".into()),
            (Rabbit, "queue", "mails".into(), Role::Producer, "convertAndSend".into()),
        ]);
        assert_eq!(names(KAFKA).len(), 5);

        let host = setup();
        host.add_file("src/main/kotlin/com/acme/orders/KtEvents.kt", KOTLIN);
        let mut m = Messaging::default();
        let kt = Document {
            path: "/project/src/main/kotlin/com/acme/orders/KtEvents.kt".into(),
            language: "kotlin".into(),
            version: 1,
            text: KOTLIN.into(),
        };
        let tips: Vec<_> = m.line_markers(&kt).into_iter().map(|x| (x.line, x.tooltip)).collect();
        assert!(tips.iter().any(|(l, t)| *l == 4 && t.contains("OrderEvents.java:8 (send), KtEvents.kt:15 (send)")), "{tips:?}");
        assert!(tips.iter().any(|(l, t)| *l == 14 && t.contains("OrderEvents.java:13 (@KafkaListener), KtEvents.kt:5 (@KafkaListener)")), "{tips:?}");
    }

    #[test]
    fn kotlin_templates_and_type_only_receivers() {
        let src = r#"class A(private val template: KafkaTemplate<String, String>, private val tpl: RabbitTemplate) {
    private val outbox: JmsTemplate = build()
    fun f(prefix: String, env: String) {
        template.send("$prefix-orders", "x")
        template.send("orders-$env", "x")
        template.send("typed-topic", "x")
        tpl.convertAndSend("typed-queue", "x")
        outbox.convertAndSend("typed-dest", "x")
    }
}
"#;
        let found: Vec<_> = parse("A.kt", src).unwrap().into_iter().map(|e| (e.broker, e.name)).collect();
        assert_eq!(found, [
            (Broker::Kafka, "typed-topic".to_owned()),
            (Broker::Rabbit, "typed-queue".to_owned()),
            (Broker::Jms, "typed-dest".to_owned()),
        ]);
    }

    #[test]
    fn finds_kafka_endpoints() {
        use Broker::Kafka;
        assert_eq!(
            names(KAFKA),
            [
                (Kafka, "topic", "orders".into(), Role::Producer, "send".into()),
                (Kafka, "topic", "audit".into(), Role::Producer, "new ProducerRecord".into()),
                (Kafka, "topic", "orders".into(), Role::Consumer, "@KafkaListener".into()),
                (Kafka, "topic", "invoices".into(), Role::Producer, "@SendTo".into()),
                (Kafka, "topic", "orders".into(), Role::Declaration, "TopicBuilder.name".into()),
            ]
        );
    }

    #[test]
    fn finds_rabbit_and_jms_endpoints() {
        use Broker::{Jms, Rabbit};
        assert_eq!(
            names(RABBIT),
            [
                (Rabbit, "queue", "mails".into(), Role::Producer, "convertAndSend".into()),
                (Rabbit, "exchange", "events".into(), Role::Producer, "convertAndSend".into()),
                (Rabbit, "queue", "direct-q".into(), Role::Producer, "convertAndSend".into()),
                (Rabbit, "queue", "delayed".into(), Role::Producer, "convertAndSend".into()),
                (Rabbit, "queue", "mail-events".into(), Role::Consumer, "@RabbitListener".into()),
                (Rabbit, "exchange", "events".into(), Role::Consumer, "@RabbitListener".into()),
                (Rabbit, "queue", "mails".into(), Role::Consumer, "@RabbitListener".into()),
                (Rabbit, "exchange", "replies".into(), Role::Producer, "@SendTo".into()),
                (Rabbit, "queue", "mails".into(), Role::Declaration, "QueueBuilder.durable".into()),
                (Rabbit, "exchange", "events".into(), Role::Declaration, "new TopicExchange".into()),
                (Jms, "destination", "audit".into(), Role::Consumer, "@JmsListener".into()),
                (Jms, "destination", "audit.out".into(), Role::Producer, "convertAndSend".into()),
            ]
        );
    }

    fn setup() -> MockHost {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/orders/OrderEvents.java", KAFKA);
        host.add_file("src/main/java/com/acme/mail/Mailer.java", RABBIT);
        host
    }

    fn doc(path: &str, text: &str) -> Document {
        Document {
            path: format!("/project/src/main/java/com/acme/{path}"),
            language: "java".into(),
            version: 1,
            text: text.into(),
        }
    }

    #[test]
    fn links_consumers_and_producers() {
        let _host = setup();
        let mut m = Messaging::default();
        let d = doc("orders/OrderEvents.java", KAFKA);
        let markers = m.line_markers(&d);
        let found: Vec<_> = markers.iter().map(|m| (m.line, m.tooltip.as_str())).collect();
        assert_eq!(
            found,
            [
                (7, "Kafka topic orders: OrderEvents.java:13 (@KafkaListener)"),
                (12, "Kafka topic orders: OrderEvents.java:8 (send), OrderEvents.java:17 (TopicBuilder.name)"),
            ]
        );
        assert_eq!(markers[1].target.as_ref().unwrap().range.start, Position::new(7, 23));

        let rabbit = m.line_markers(&doc("mail/Mailer.java", RABBIT));
        let tips: Vec<_> = rabbit.iter().map(|m| m.line).collect();
        assert_eq!(tips, [7, 8, 13, 16], "producers to mails/events and their consumers");

        let targets = m.definition(&d, Position::new(16, 61));
        assert_eq!(targets.len(), 2, "the producer and the consumer of `orders`");
    }

    #[test]
    fn completes_known_names_per_broker() {
        let _host = setup();
        let mut m = Messaging::default();
        let typing = KAFKA.replace("streamBridge.send(\"orders-out\", o);", "kafkaTemplate.send(\"o\", o);");
        let items = m.completion(&doc("orders/OrderEvents.java", &typing), Position::new(8, 29), None);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["orders", "audit", "invoices"]);
    }

    #[test]
    fn messaging_view_groups_by_broker_and_destination() {
        let host = setup();
        let mut m = Messaging::default();
        let view = m.render(VIEW, None, &host.invocation()).unwrap();
        let Body::Tree(items) = view.body else { panic!("tree expected") };
        let roots: Vec<_> = items.iter().filter(|i| i.parent.is_none()).map(|i| i.label.as_str()).collect();
        assert_eq!(roots, ["Kafka", "RabbitMQ", "JMS"]);
        let orders: Vec<_> = items.iter().filter(|i| i.parent.as_deref() == Some("d:Kafka:topic:orders")).collect();
        assert_eq!(
            orders.iter().map(|i| i.detail.as_deref().unwrap()).collect::<Vec<_>>(),
            ["consumer · @KafkaListener", "producer · send", "declaration · TopicBuilder.name"]
        );
        assert_eq!(orders.iter().map(|i| i.icon.as_deref().unwrap()).collect::<Vec<_>>(), [
            "vcs-incoming",
            "vcs-outgoing",
            "spring-config"
        ]);
        let destination = |key: &str| {
            let d = items.iter().find(|i| i.key == key).unwrap();
            (d.detail.as_deref().unwrap(), d.icon.as_deref().unwrap())
        };
        assert_eq!(destination("d:Kafka:topic:orders"), ("topic · 1 producer · 1 consumer", "send"));
        assert_eq!(destination("d:Kafka:topic:invoices"), ("topic · 1 producer · 0 consumers", "warning"));
        assert_eq!(view.status.as_deref(), Some("11 destinations"));
        assert!(host.refreshed_views().is_empty(), "the scan is complete");
        m.handle(VIEW, ViewEvent::Activate(orders[1].key.clone()), &host.invocation()).unwrap();
        assert_eq!(
            host.opened_locations(),
            [("/project/src/main/java/com/acme/orders/OrderEvents.java".into(), 7, 23)]
        );
    }

    #[cfg(not(feature = "embedded"))]
    #[test]
    fn refresh_keeps_a_scan_in_progress() {
        let host = setup();
        let mut m = Messaging::default();
        let project = host.invocation().project().unwrap();
        m.index.get(&project, Duration::ZERO).update("/project/src/main/java/com/acme/mail/Mailer.java", RABBIT);
        m.handle(VIEW, ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
        assert_eq!(m.index.get(&project, Duration::ZERO).files().len(), 1, "not reset");
        assert_eq!(host.refreshed_views(), [VIEW]);

        m.index.get(&project, COMMAND_BUDGET);
        m.handle(VIEW, ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
        assert!(m.index.get(&project, Duration::ZERO).files().is_empty(), "a complete index starts over");
    }

    #[test]
    fn reads_simple_spel_destinations() {
        assert_eq!(spel_destination("#{'orders'}").as_deref(), Some("orders"));
        assert_eq!(spel_destination("#{'a' + \"-b\"}").as_deref(), Some("a-b"));
        assert_eq!(spel_destination("#{'${app.topic}'}").as_deref(), Some("${app.topic}"));
        assert_eq!(spel_destination("#{T(com.acme.Queues).ORDERS}"), Some(format!("{CONSTANT}Queues.ORDERS")));
        for dynamic in ["#{@names.next()}", "#{'a'.concat(x)}", "#{T(Q).A + 'x'}", "#{x}", "pre#{'a'}", "#{'a' + b}", "#{}", "#{cond ? 'a' : 'b'}"] {
            assert_eq!(spel_destination(dynamic), None, "{dynamic}");
        }
    }

    #[test]
    fn spel_destinations_become_endpoints() {
        let host = MockHost::install();
        host.add_file("src/main/resources/application.properties", "app.topic=events\n");
        host.add_file("src/main/java/com/acme/Queues.java", "class Queues { static final String ORDERS = \"orders.in\"; }");
        let listeners = "class L {\n    @KafkaListener(topics = \"#{'${app.topic}'}\") void a(String s) {}\n    @RabbitListener(queues = \"#{T(com.acme.Queues).ORDERS}\") void b(String s) {}\n    @JmsListener(destination = \"#{@names.next()}\") void c(String s) {}\n}\n";
        let producers = "class P {\n    void go() {\n        kafkaTemplate.send(\"events\", \"x\");\n        rabbitTemplate.convertAndSend(\"orders.in\", \"x\");\n        jmsTemplate.convertAndSend(\"anything\", \"x\");\n    }\n}\n";
        host.add_file("src/main/java/L.java", listeners);
        host.add_file("src/main/java/P.java", producers);
        let mut m = Messaging::default();
        let l = Document { path: "/project/src/main/java/L.java".into(), language: "java".into(), version: 1, text: listeners.into() };
        let lines: Vec<_> = m.line_markers(&l).iter().map(|k| (k.line, k.tooltip.clone())).collect();
        assert_eq!(
            lines,
            [
                (1, "Kafka topic events: P.java:3 (send)".to_owned()),
                (2, "RabbitMQ queue orders.in: P.java:4 (convertAndSend)".to_owned()),
            ]
        );
    }

    #[test]
    fn expands_placeholders_to_depth_two() {
        let props: Props = [("a.b", "x"), ("deep", "${a.b}"), ("deeper", "${deep}")]
            .into_iter()
            .map(|(k, v)| (config::canonical(k), v.to_owned()))
            .collect();
        let at = |t: &str| expand(t, &props, 2);
        assert_eq!(at("${a.b}"), Some("x".into()));
        assert_eq!(at("p-${A.B}-s"), Some("p-x-s".into()));
        assert_eq!(at("${missing:dflt}"), Some("dflt".into()));
        assert_eq!(at("${missing:${a.b}}"), Some("x".into()));
        assert_eq!(at("${deep}"), Some("x".into()));
        assert_eq!(at("${deeper}"), None, "three levels");
        assert_eq!(at("${missing}"), None);
    }

    #[test]
    fn listener_topic_placeholder_resolves_from_application_properties() {
        let host = MockHost::install();
        let listeners = r#"class L {
    @KafkaListener(topics = "${app.topic-name}") void a(String s) {}
    @RabbitListener(queues = "${app.queue:fallback-q}") void b(String s) {}
    @JmsListener(destination = "${app.dest}") @SendTo("${app.reply}") String c(String s) { return s; }
    @KafkaListener(topics = "${app.nope}") void d(String s) {}
    @KafkaListener(topics = "${app.nope:${app.other}}") void e(String s) {}
}
"#;
        let producers = r#"class P {
    void go() {
        kafkaTemplate.send("orders", "x");
        rabbitTemplate.convertAndSend("fallback-q", "x");
        jmsTemplate.convertAndSend("audit", "x");
        kafkaTemplate.send("other-topic", "x");
        kafkaTemplate.send("${app.nope}", "x");
    }
    @JmsListener(destination = "invoices") void inv(String s) {}
}
"#;
        host.add_file("src/main/java/L.java", listeners);
        host.add_file("src/main/java/P.java", producers);
        host.add_file("src/main/resources/application-dev.properties", "app.topic-name=dev-orders\napp.dest=dev-audit\n");
        host.add_file(
            "src/main/resources/application.yml",
            "app:\n  topicName: orders\n  dest: audit\n  reply: invoices\n  other: other-topic\n",
        );
        let mut m = Messaging::default();
        let l = Document {
            path: "/project/src/main/java/L.java".into(),
            language: "java".into(),
            version: 1,
            text: listeners.into(),
        };
        let lines: Vec<_> = m.line_markers(&l).iter().map(|k| (k.line, k.tooltip.clone())).collect();
        assert_eq!(
            lines,
            [
                (1, "Kafka topic orders: P.java:3 (send)".to_owned()),
                (2, "RabbitMQ queue fallback-q: P.java:4 (convertAndSend)".to_owned()),
                (3, "JMS destination audit: P.java:5 (convertAndSend)".to_owned()),
                (3, "JMS destination invoices: P.java:9 (@JmsListener)".to_owned()),
                (5, "Kafka topic other-topic: P.java:6 (send)".to_owned()),
            ],
            "the unresolvable `${{app.nope}}` has no marker; application.yml beats the dev profile"
        );
        let p = Document {
            path: "/project/src/main/java/P.java".into(),
            language: "java".into(),
            version: 1,
            text: producers.into(),
        };
        let reply: Vec<_> = m.line_markers(&p).iter().map(|k| k.line).collect();
        assert_eq!(reply, [2, 3, 4, 5, 8], "producers to resolved consumers; `invoices` meets the resolved @SendTo");
    }

    #[test]
    fn xml_listener_container_links_to_bean() {
        let host = MockHost::install();
        let xml = r#"<beans xmlns="http://www.springframework.org/schema/beans"
       xmlns:jms="http://www.springframework.org/schema/jms"
       xmlns:rabbit="http://www.springframework.org/schema/rabbit">
    <jms:listener-container connection-factory="cf">
        <jms:listener destination="audit" ref="auditBean" method="handle"/>
    </jms:listener-container>
    <rabbit:listener-container connection-factory="rcf">
        <rabbit:listener queue-names="mails, other-q" ref="mailBean" method="onMail"/>
    </rabbit:listener-container>
</beans>
"#;
        let java = r#"class P {
    void go() {
        jmsTemplate.convertAndSend("audit", "x");
        rabbitTemplate.convertAndSend("mails", "x");
    }
}
"#
        .to_owned();
        host.add_file("src/main/resources/beans.xml", xml);
        host.add_file("src/main/java/P.java", &java);
        let mut m = Messaging::default();
        let x = Document {
            path: "/project/src/main/resources/beans.xml".into(),
            language: "xml".into(),
            version: 1,
            text: xml.into(),
        };
        let markers = m.line_markers(&x);
        let found: Vec<_> = markers.iter().map(|k| (k.line, k.tooltip.clone())).collect();
        assert_eq!(
            found,
            [
                (4, "JMS destination audit: P.java:3 (convertAndSend)".to_owned()),
                (7, "RabbitMQ queue mails: P.java:4 (convertAndSend)".to_owned()),
            ]
        );
        let j = Document { path: "/project/src/main/java/P.java".into(), language: "java".into(), version: 1, text: java };
        let back: Vec<_> = m.line_markers(&j).iter().map(|k| (k.line, k.tooltip.clone())).collect();
        assert_eq!(back[0].1, "JMS destination audit: beans.xml:5 (<jms:listener>)");
        assert_eq!(back[1].1, "RabbitMQ queue mails: beans.xml:8 (<rabbit:listener>)");
    }

    const QUEUES: &str = "package com.acme.mail;\n\npublic final class Queues {\n    public static final String MAILS = \"mails\";\n    public static final String EVENTS = \"mail-\" + \"events\";\n}\n";
    const CONSTANT_USER: &str = r#"package com.acme.mail;

class Sender {
    @RabbitListener(queues = {Queues.MAILS, Queues.GHOST})
    void onMail(Mail m) {}

    void publish() {
        rabbitTemplate.convertAndSend(Queues.EVENTS, "x");
        kafkaTemplate.send(Topics.UNKNOWN, "x");
    }
}
"#;

    #[test]
    fn destinations_named_by_constants_link_to_literals() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/mail/Queues.java", QUEUES);
        host.add_file("src/main/java/com/acme/mail/Sender.java", CONSTANT_USER);
        host.add_file("src/main/java/com/acme/mail/Other.java", "class Other { void go() { rabbit.convertAndSend(\"mails\", \"x\"); } }");
        let mut m = Messaging::default();
        let d = doc("mail/Sender.java", CONSTANT_USER);
        let all: Vec<_> = m.endpoints(&d).into_iter().map(|(p, e)| (file_name(&p).to_owned(), e.name, e.role)).collect();
        assert!(all.contains(&("Sender.java".into(), "mails".into(), Role::Consumer)), "{all:?}");
        assert!(all.contains(&("Sender.java".into(), "mail-events".into(), Role::Producer)));
        assert!(!all.iter().any(|(_, n, _)| n.contains("GHOST") || n.contains("UNKNOWN") || n.contains(CONSTANT)), "unresolved constants are dropped");
        let at = openidea_plugin::text::position(CONSTANT_USER, CONSTANT_USER.find("Queues.MAILS").unwrap() + 3);
        let linked: Vec<_> = m.definition(&d, at).iter().map(|l| file_name(&l.path).to_owned()).collect();
        assert_eq!(linked, ["Other.java"]);
    }

    const LISTENERS_XML: &str = r#"<beans xmlns="http://www.springframework.org/schema/beans"
       xmlns:jms="http://www.springframework.org/schema/jms"
       xmlns:rabbit="http://www.springframework.org/schema/rabbit">
    <bean id="mailBean" class="com.acme.mail.MailHandler"/>
    <jms:listener-container acknowledge="sometimes" cache="auto">
        <jms:listener ref="mailBean" method="missing"/>
        <jms:listener destination="audit" ref="ghostBean"/>
    </jms:listener-container>
    <rabbit:listener-container acknowledge="manual">
        <rabbit:listener queue-names="q" ref="mailBean" method="handle"/>
        <rabbit:listener queue-names="q" ref="${bean}" method="x"/>
        <rabbit:listener queue-names="q"/>
    </rabbit:listener-container>
    <rabbit:topic-exchange auto-delete="maybe"/>
</beans>
"#;
    const HANDLER: &str = "package com.acme.mail;\n\npublic class MailHandler {\n    public void handle(String m) {}\n}\n";

    fn xml_doc(text: &str) -> Document {
        Document { path: "/project/src/main/resources/listeners.xml".into(), language: "xml".into(), version: 1, text: text.into() }
    }

    fn listener_host() -> MockHost {
        let host = MockHost::install();
        host.add_file("src/main/resources/listeners.xml", LISTENERS_XML);
        host.add_file("src/main/java/com/acme/mail/MailHandler.java", HANDLER);
        host
    }

    #[cfg(not(feature = "embedded"))]
    #[test]
    fn xml_listener_elements_are_checked_against_the_tables() {
        let host = listener_host();
        let mut m = Messaging::default();
        m.line_markers(&xml_doc(LISTENERS_XML));
        let found: Vec<_> = host.diagnostics()["/project/src/main/resources/listeners.xml"]
            .iter()
            .map(|d| (d.range.start.line, d.code.clone().unwrap(), d.severity))
            .collect();
        assert_eq!(found, [
            (11, "missing-attribute".to_owned(), Severity::Warning),
            (13, "missing-attribute".into(), Severity::Warning),
            (13, "invalid-attribute-value".into(), Severity::Warning),
            (4, "invalid-attribute-value".into(), Severity::Warning),
            (5, "missing-attribute".into(), Severity::Warning),
            (6, "unresolved-reference".into(), Severity::Info),
            (5, "unresolved-listener-method".into(), Severity::Warning),
        ]);
    }

    #[test]
    fn xml_rabbit_booleans_are_checked() {
        let _host = listener_host();
        let xml = "<beans xmlns:rabbit=\"http://www.springframework.org/schema/rabbit\">\n<rabbit:connection-factory id=\"cf\" publisher-returns=\"yes\"/>\n<rabbit:admin auto-startup=\"true\"/>\n</beans>";
        let found = listener_xml::problems(xml, &spring_text::index::Index::new(&[], listener_xml::parse));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code.as_deref(), Some("invalid-attribute-value"));
    }

    #[test]
    fn xml_listener_ref_and_method_navigate_and_complete() {
        let _host = listener_host();
        let mut m = Messaging::default();
        let d = xml_doc(LISTENERS_XML);
        let after = |needle: &str| openidea_plugin::text::position(LISTENERS_XML, LISTENERS_XML.find(needle).unwrap() + needle.len());
        let bean = m.definition(&d, after("queue-names=\"q\" ref=\"mail"));
        assert_eq!((bean.len(), bean[0].range.start.line), (1, 3));
        let method = m.definition(&d, after("method=\"han"));
        assert!(method[0].path.ends_with("MailHandler.java"));
        let labels = |items: Vec<CompletionItem>| items.into_iter().map(|i| i.label).collect::<Vec<_>>();
        assert_eq!(labels(m.completion(&d, after("method=\"han"), None)), ["handle"]);
        assert!(labels(m.completion(&d, after("ref=\"mail"), None)).contains(&"mailBean".to_owned()));
        assert!(m.completion(&d, after("<rabbit:listener-container acknowledge=\"ma"), None).is_empty());
    }

    #[test]
    fn xml_listener_checks_are_quiet_for_valid_files() {
        let host = listener_host();
        let ok = "<beans xmlns=\"http://www.springframework.org/schema/beans\"\n       xmlns:rabbit=\"http://www.springframework.org/schema/rabbit\">\n<bean id=\"mailBean\" class=\"com.acme.mail.MailHandler\"/>\n<rabbit:listener-container acknowledge=\"auto\"><rabbit:listener queues=\"q\" ref=\"mailBean\" method=\"handle\"/></rabbit:listener-container>\n</beans>\n";
        Messaging::default().line_markers(&xml_doc(ok));
        assert!(host.diagnostics().get("/project/src/main/resources/listeners.xml").is_none_or(|found| found.is_empty()));
    }

    #[cfg(feature = "embedded")]
    #[test]
    fn refresh_goes_to_the_owner_window_when_embedded() {
        let host = MockHost::install();
        refresh("some-view");
        assert_eq!(host.refreshed_views(), ["endpoints"]);
    }

    #[test]
    fn applies_follows_the_root_build_file() {
        let host = MockHost::install();
        let project = &host::projects().unwrap()[0];
        assert!(!applies(project), "no build file");
        host.add_file("pom.xml", "<project><artifactId>commons-lang3</artifactId></project>");
        assert!(!applies(project));
        host.add_file("pom.xml", "<project><artifactId>spring-kafka</artifactId></project>");
        assert!(applies(project));
        host.add_file("pom.xml", "<project><modules><module>app</module></modules></project>");
        assert!(applies(project), "multi-module is unsure");
    }
}
