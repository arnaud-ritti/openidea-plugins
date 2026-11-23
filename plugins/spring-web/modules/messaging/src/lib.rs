//! Spring Messaging Tools for OpenIDEA: links the consumers, producers and
//! declarations of Kafka topics, RabbitMQ queues and exchanges and JMS
//! destinations in a Java project by destination name. Text-based.

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Error, Invocation, Location, Plugin, Position, Range, host};
use spring_text::constants::Constants;
#[cfg(test)]
use spring_text::index::COMMAND_BUDGET;
use spring_text::index::{Index, Indexes, VIEW_BUDGET};
use spring_text::java::{self, Annotation, Arg, Call, CallArg, JavaFile, Str, Tok};
use spring_text::tree::Expansion;
use spring_text::{config, xml};
use spring_text::{parse_source, range, source_calls};
use std::collections::{BTreeMap, BTreeSet, HashMap};

mod listener_xml;
mod destination_docs;
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
    /// Named by a bean reference rather than a literal: navigable, but not a place to complete names.
    pub reference: bool,
    /// A routing key bound to a topic exchange: `*` and `#` are wildcards. Otherwise the key is literal.
    pub wildcards: bool,
}

impl Endpoint {
    fn same_destination(&self, o: &Endpoint) -> bool {
        if self.broker != o.broker || self.space != o.space {
            return false;
        }
        self.name == o.name || (self.space == ROUTING_KEY && (self.binds_key_of(o) || o.binds_key_of(self)))
    }

    /// A binding whose topic pattern (`order.#`) matches the literal key a producer sends (`order.created`).
    fn binds_key_of(&self, sent: &Endpoint) -> bool {
        if self.role != Role::Declaration || sent.role != Role::Producer {
            return false;
        }
        match (self.name.split_once('/'), sent.name.split_once('/')) {
            (Some((x, pattern)), Some((sent_x, key))) => x == sent_x && (self.wildcards && topic_matches(pattern, key) || pattern == key),
            _ => false,
        }
    }

    fn holds(&self, at: usize) -> bool {
        self.at <= at && at <= self.end
    }
}

const ROUTING_KEY: &str = "routing key";
const UNRESOLVED_BINDING: &str = "binding unresolved";

/// AMQP topic matching: `*` is one word, `#` zero or more.
fn topic_matches(pattern: &str, key: &str) -> bool {
    let mut words: Vec<&str> = Vec::new();
    for word in pattern.split('.') {
        if word != "#" || words.last() != Some(&"#") {
            words.push(word);
        }
    }
    let key: Vec<&str> = key.split('.').collect();
    // reach[j]: the pattern words consumed so far match the first j key words.
    let mut reach = vec![false; key.len() + 1];
    reach[0] = true;
    for word in words {
        if word == "#" {
            let mut any = false;
            for slot in reach.iter_mut() {
                any |= *slot;
                *slot = any;
            }
            continue;
        }
        for j in (1..=key.len()).rev() {
            reach[j] = reach[j - 1] && (word == "*" || word == key[j - 1]);
        }
        reach[0] = false;
    }
    reach[key.len()]
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
    a.text.contains("->") || a.text.contains("::") || a.text.to_lowercase().contains("correlation") || a.text.contains("PostProcessor")
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
    let ident =
        |p: &str| p.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && p.chars().all(|c| c.is_alphanumeric() || c == '_');
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
        self.out.push(Endpoint { broker, space, name, role, via: via.to_owned(), at: s.at, end: s.end, range, reference: false, wildcards: false });
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
                self.add(broker, "exchange", Role::Producer, "@SendTo", &Str { value: exchange.to_owned(), at: s.at, end });
            }
            None => self.add(broker, "queue", Role::Producer, "@SendTo", s),
        }
    }

    /// `template.convertAndSend("exchange", "key", msg)` on a receiver proved a Spring AMQP template: the literal key.
    fn routing_key_producer(&mut self, c: &Call, file: &JavaFile) {
        let Some(receiver) = c.receiver.as_deref() else { return };
        let Some(ty) = amqp_template(file, receiver) else { return };
        let [exchange, key, rest @ ..] = c.args.as_slice() else { return };
        let (Some(exchange), Some(key)) = (exchange.string.as_ref(), key.string.as_ref()) else { return };
        // RabbitMessagingTemplate also has `(destination, payload, Map headers)`.
        let ambiguous = rest.len() == 1 && (is_extra(&rest[0]) || (ty == "RabbitMessagingTemplate" && ["Map", "eaders"].iter().any(|h| rest[0].text.contains(h))));
        let plain = |v: &str| !v.is_empty() && !v.contains(['$', '#', '{', '/', CONSTANT]);
        if ambiguous || rest.is_empty() || !plain(&exchange.value) || !plain(&key.value) {
            return;
        }
        let name = Str { value: format!("{}/{}", exchange.value, key.value), at: key.at, end: key.end };
        self.add(Broker::Rabbit, ROUTING_KEY, Role::Producer, &c.name, &name);
    }

    fn unresolved_binding(&mut self, at: usize, end: usize) {
        let range = range(self.text, at, end);
        let (broker, space) = (Broker::Rabbit, UNRESOLVED_BINDING);
        self.out.push(Endpoint { broker, space, name: String::new(), role: Role::Declaration, via: UNRESOLVED_BINDING.to_owned(), at, end, range, reference: true, wildcards: false });
    }

    fn call(&mut self, c: &Call, file: &JavaFile) {
        if c.is_new && c.name == "Binding" {
            self.unresolved_binding(c.at, c.at + c.name.len());
        }
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
            ("QueueBuilder", "durable" | "nonDurable") => self.add(Broker::Rabbit, "queue", Role::Declaration, &via, &first),
            ("ExchangeBuilder", "topicExchange" | "directExchange" | "fanoutExchange" | "headersExchange") => {
                self.add(Broker::Rabbit, "exchange", Role::Declaration, &via, &first)
            }
            (_, "send" | "convertAndSend" | "convertSendAndReceive" | "sendAndReceive") => {
                match Broker::hinted_by(receiver).or_else(|| declared_type(file, receiver).and_then(Broker::hinted_by)) {
                    Some(Broker::Kafka) if c.name == "send" => self.add(Broker::Kafka, "topic", Role::Producer, &c.name, &first),
                    Some(Broker::Rabbit) => {
                        if let Some((space, s)) = rabbit_target(c) {
                            self.add(Broker::Rabbit, space, Role::Producer, &c.name, &s);
                            if space == "exchange" {
                                self.routing_key_producer(c, file);
                            }
                        }
                    }
                    Some(Broker::Jms) if c.args.len() >= 2 => self.add(Broker::Jms, "destination", Role::Producer, &c.name, &first),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

/// `BindingBuilder.bind(queue).to(exchange).with("key")`: the queue and exchange it names, through
/// same-file bean methods that declare exactly one literal of that kind, and the literal routing key.
fn bindings(collector: &mut Collector, file: &JavaFile, calls: &[Call]) {
    let declared = |ident: &str, space: &str| {
        let ident = ident.trim().trim_start_matches("this.");
        let ident = ident.strip_suffix("()").unwrap_or(ident);
        let (from, to) = file.types.iter().flat_map(|t| t.methods()).find(|m| m.name == ident)?.body?;
        let mut inside = collector.out.iter().filter(|e| e.role == Role::Declaration && e.space == space && e.at >= from && e.end <= to);
        let found = inside.next()?;
        inside.next().is_none().then(|| (found.name.clone(), found.via.clone()))
    };
    let at = |arg: &CallArg, space, name: String, via: &str, reference| Endpoint {
        broker: Broker::Rabbit,
        space,
        name,
        role: Role::Declaration,
        via: via.to_owned(),
        at: arg.at,
        end: arg.end,
        range: range(collector.text, arg.at, arg.end),
        reference,
        wildcards: false,
    };
    let mut found = Vec::new();
    let mut unresolved = Vec::new();
    for (i, bind) in calls.iter().enumerate().filter(|(_, c)| c.name == "bind" && c.receiver.as_deref() == Some("BindingBuilder")) {
        let chain: Vec<_> = calls[i + 1..].iter().take_while(|c| c.chained).collect();
        let (Some(queue), Some(to)) = (bind.args.first(), chain.first().filter(|c| c.name == "to")) else {
            unresolved.push(bind.at);
            continue;
        };
        let Some(exchange) = to.args.first() else { continue };
        let (Some((queue_name, _)), Some((exchange_name, exchange_via))) = (declared(&queue.text, "queue"), declared(&exchange.text, "exchange")) else {
            unresolved.push(bind.at);
            continue;
        };
        found.push(at(queue, "queue", queue_name, "BindingBuilder.bind", true));
        found.push(at(exchange, "exchange", exchange_name.clone(), "BindingBuilder.to", true));
        let plain = |v: &str| !v.contains(CONSTANT) && !v.contains("${") && !v.contains("#{");
        let Some(key) = chain.get(1).filter(|c| c.name == "with").and_then(|c| c.args.first()) else { continue };
        match key.string.as_ref().filter(|k| plain(&k.value) && plain(&exchange_name) && !k.value.is_empty() && !exchange_name.contains('/')) {
            Some(key_str) => {
                let name = format!("{exchange_name}/{}", key_str.value);
                let wildcards = exchange_via.to_ascii_lowercase().contains("topic");
                found.push(Endpoint { at: key_str.at, end: key_str.end, range: range(collector.text, key_str.at, key_str.end), wildcards, ..at(key, ROUTING_KEY, name, "BindingBuilder.with", false) });
            }
            None => unresolved.push(bind.at),
        }
    }
    collector.out.extend(found);
    for at in unresolved {
        collector.unresolved_binding(at, at + "bind".len());
    }
}

const AMQP_TEMPLATES: [(&str, &str); 3] = [
    ("RabbitTemplate", "org.springframework.amqp.rabbit.core.RabbitTemplate"),
    ("AmqpTemplate", "org.springframework.amqp.core.AmqpTemplate"),
    ("RabbitMessagingTemplate", "org.springframework.amqp.rabbit.core.RabbitMessagingTemplate"),
];

/// The Spring AMQP template type of a same-file field or parameter, when exactly imported.
fn amqp_template(file: &JavaFile, name: &str) -> Option<&'static str> {
    let declared = declared_type(file, name)?;
    let (simple, fqn) = AMQP_TEMPLATES.iter().find(|(n, _)| *n == declared)?;
    let package = fqn.rsplit_once('.').map_or("", |(p, _)| p);
    let imported = file.imports.iter().any(|i| !i.is_static && (i.path == *fqn || i.path.strip_suffix(".*") == Some(package)));
    imported.then_some(*simple)
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
        if e.local() != "listener" && e.local() != "binding" {
            continue;
        }
        let Some(namespace) = xml::namespace(&elements, i) else { continue };
        if e.local() == "binding" {
            if namespace.ends_with("/schema/rabbit") {
                collector.unresolved_binding(e.at, e.at + e.name.len());
            }
            continue;
        }
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
        if !text.contains("listener") && !text.contains("binding") {
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
    let calls = source_calls(path, text);
    for c in &calls {
        collector.call(c, &file);
    }
    bindings(&mut collector, &file, &calls);
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
    let keys: Vec<_> =
        config::keys(path, text).into_iter().filter(|k| !k.value.is_empty()).map(|k| (config::canonical(&k.key), k.value)).collect();
    (!keys.is_empty()).then_some(keys)
}

/// Value of a key defined with differing values: the profile that wins is unknowable, and `expand` can't close it, so it resolves to nothing.
const AMBIGUOUS: &str = "${";

fn props_of<'a>(files: impl Iterator<Item = (&'a str, &'a Vec<(String, String)>)>) -> Props {
    let mut props = Props::new();
    for (key, value) in files.flat_map(|(_, keys)| keys) {
        match props.get(key) {
            Some(known) if known != value => {
                props.insert(key.clone(), AMBIGUOUS.to_owned());
            }
            Some(_) => {}
            None => {
                props.insert(key.clone(), value.clone());
            }
        }
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
    if e.space == UNRESOLVED_BINDING {
        return None;
    }
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

/// The host's navigate arrow for every link this module draws: explicit so same-line markers
/// group predictably, and apart from the web route icon so they never merge with its URL markers.
const MARKER_ICON: &str = "navigate";

/// Most markers one line gets; the rest are named in the tooltip.
const MAX_LINE_TARGETS: usize = 200;

/// One marker per target on `line`: the IDE folds same-line, same-icon markers into one gutter
/// button that jumps to a single target or opens its chooser for several.
fn per_target(line: u32, tooltip: &str, targets: Vec<Location>) -> Vec<LineMarker> {
    let total = targets.len();
    let tooltip = if total > MAX_LINE_TARGETS { format!("{tooltip} (first {MAX_LINE_TARGETS} of {total})") } else { tooltip.to_owned() };
    targets.into_iter().take(MAX_LINE_TARGETS).map(|t| LineMarker { line, icon: Some(MARKER_ICON.into()), tooltip: tooltip.clone(), target: Some(t) }).collect()
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

/// What one Java, Kotlin or XML file says to this module, read once.
#[derive(Clone)]
pub struct Facts {
    endpoints: Vec<Endpoint>,
    constants: Vec<(String, String)>,
    beans: Option<listener_xml::Info>,
}

fn facts(path: &str, text: &str) -> Option<Facts> {
    let endpoints = parse(path, text);
    let constants = parse_constants(path, text);
    let beans = listener_xml::parse(path, text);
    if endpoints.is_none() && constants.is_none() && beans.is_none() {
        return None;
    }
    Some(Facts { endpoints: endpoints.unwrap_or_default(), constants: constants.unwrap_or_default(), beans })
}

pub struct Messaging {
    index: Indexes<Facts>,
    config: Indexes<Vec<(String, String)>>,
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
            index: Indexes::new(&["**/*.java", "**/*.kt", "**/*.xml"], facts),
            config: Indexes::new(&CONFIG_GLOBS, parse_config),
            expansion: Expansion::default(),
        }
    }
}

impl Messaging {
    /// Every endpoint of the document's project cached so far, by file.
    fn endpoints(&mut self, doc: &Document) -> Vec<(String, Endpoint)> {
        let props = self.config.for_cached_document(&doc.path, &doc.text).map(|i| props_of(i.iter())).unwrap_or_default();
        let Some(index) = self.index.for_cached_document(&doc.path, &doc.text) else { return Vec::new() };
        let constants = constant_values(index);
        index
            .iter()
            .flat_map(|(p, f)| f.endpoints.iter().filter_map(|e| resolved(e.clone(), &props, &constants)).map(move |e| (p.to_owned(), e)))
            .collect()
    }
}

fn constant_values(index: &Index<Facts>) -> HashMap<String, String> {
    index.iter().flat_map(|(_, f)| f.constants.iter().cloned()).collect()
}

/// The endpoints of `doc` and the others of the same destination, by role.
fn links<'a>(doc: &Document, all: &'a [(String, Endpoint)]) -> Vec<(&'a Endpoint, Vec<&'a (String, Endpoint)>)> {
    all.iter()
        .filter(|(p, _)| *p == doc.path)
        .map(|(_, e)| {
            let mut others: Vec<_> = all.iter().filter(|(p, o)| o.same_destination(e) && !(*p == doc.path && o.at == e.at)).collect();
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
        let Some(beans) = self.index.for_cached_document(&doc.path, &doc.text) else { return Vec::new() };
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
        if !cfg!(feature = "embedded") && doc.language == "xml" {
            let problems = match self.index.for_cached_document(&doc.path, &doc.text) {
                Some(beans) => listener_xml::problems(&doc.text, beans),
                None => Vec::new(),
            };
            publish(&doc.path, &problems);
        }
        let all = self.endpoints(doc);
        links(doc, &all)
            .into_iter()
            .flat_map(|(e, others)| {
                let wanted = |o: &Endpoint| match e.role {
                    Role::Consumer => o.role != Role::Consumer,
                    Role::Producer => o.role == Role::Consumer || (e.space == ROUTING_KEY && o.role == Role::Declaration),
                    Role::Declaration => e.space == ROUTING_KEY && o.role == Role::Producer,
                };
                let others: Vec<_> = others.into_iter().filter(|(_, o)| wanted(o)).collect();
                let list: Vec<String> =
                    others.iter().map(|(p, o)| format!("{}:{} ({})", file_name(p), o.range.start.line + 1, o.via)).collect();
                let targets = others.iter().map(|(p, o)| location(p, o)).collect();
                per_target(e.range.start.line, &format!("{}: {}", title(e), list.join(", ")), targets)
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
        let Some(beans) = self.index.for_cached_document(&doc.path, &doc.text) else { return Vec::new() };
        match listener_xml::target_at(&doc.text, beans, at) {
            Some((listener_xml::Target::Bean, name, ..)) => listener_xml::bean_locations(beans, &name),
            Some((listener_xml::Target::Method(Some(class)), name, ..)) => listener_xml::method_locations(beans, &class, &name),
            _ => Vec::new(),
        }
    }

    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let all = self.endpoints(doc);
        let Some(here) = all.iter().find(|(p, e)| *p == doc.path && !e.reference && e.holds(at)).map(|(_, e)| e) else {
            return self.listener_completion(doc, at);
        };
        let replace = range(&doc.text, here.at, here.end);
        let mut seen = BTreeSet::new();
        all.iter()
            .filter(|(p, e)| {
                e.broker == here.broker && e.space == here.space && !(*p == doc.path && e.at == here.at) && seen.insert(&e.name)
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
        (|| -> Result<View, Error> {
            let Some(project) = ctx.project() else {
                return Ok(View::message("Open a project to see its messaging destinations."));
            };
            let config = self.config.get(&project, VIEW_BUDGET / 2);
            let config_scanning = config.scanning();
            let props = props_of(config.iter());
            let index = self.index.get(&project, VIEW_BUDGET / 2);
            let constants = constant_values(index);
            let note = spring_text::index::coverage_note(index);
            let scanning = index.scanning() || config_scanning;
            if scanning {
                refresh(view);
            }
            let refresh = Button::new("refresh", "Refresh").icon("refresh");
            let mut tree: BTreeMap<Broker, Destinations> = BTreeMap::new();
            let all: Vec<(&str, Endpoint)> = index
                .iter()
                .flat_map(|(path, f)| f.endpoints.iter().filter_map(|e| resolved(e.clone(), &props, &constants)).map(move |e| (path, e)))
                .collect();
            for (path, e) in &all {
                tree.entry(e.broker).or_default().entry((e.name.as_str(), e.space)).or_default().push((*path, e));
            }
            if tree.is_empty() {
                let text = spring_text::index::empty_message("No Kafka, RabbitMQ or JMS destinations found. Add a @KafkaListener, @RabbitListener or @JmsListener, or send with KafkaTemplate, RabbitTemplate or JmsTemplate", scanning, note.as_deref());
                return Ok(View::message(&text).button(refresh));
            }
            let mut items = Vec::new();
            let mut total = 0;
            for (broker, destinations) in tree {
                let b = format!("b:{}", broker.label());
                items.push(TreeItem::new(&b, broker.label()).detail(&destinations.len().to_string()).icon("folder").expandable(true));
                total += destinations.len();
                for ((name, space), mut uses) in destinations {
                    let d = format!("d:{}:{space}:{name}", broker.label());
                    let count = |role| uses.iter().filter(|(_, e)| e.role == role).count();
                    let (producers, consumers) = (count(Role::Producer), count(Role::Consumer));
                    let detail = format!("{space} · {} · {}", plural(producers, "producer"), plural(consumers, "consumer"));
                    let icon = if space != ROUTING_KEY && (producers > 0) != (consumers > 0) { "warning" } else { "send" };
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
            if let Some(note) = note {
                status.push_str(&format!(" · {note}"));
            }
            Ok(View::new(Body::Tree(items)).button(refresh).status(&status))
        })().map(ui_kit::actions::available)
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
                    && !self.index.get(&project, Duration::ZERO).scanning()
                {
                    self.index.reset();
                    self.config.reset();
                }
                refresh(view);
            }
            ViewEvent::Activate(key) => {
                let mut parts = key.splitn(4, ':');
                if let (Some("e"), Some(line), Some(col), Some(path)) = (parts.next(), parts.next(), parts.next(), parts.next()) {
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

const DEPENDENCY_MARKERS: [&str; 6] = ["kafka", "rabbit", "amqp", "jms", "activemq", "artemis"];

/// True when unsure (see `spring_text::build::applies`).
pub fn applies(project: &openidea_plugin::Project) -> bool {
    spring_text::build::applies(project, &DEPENDENCY_MARKERS)
}

#[cfg(not(feature = "embedded"))]
openidea_plugin::register_plugin!(Messaging, providers, views);

#[cfg(test)]
#[cfg_attr(feature = "embedded", allow(dead_code, unused_imports))]
mod tests {
    fn warmed() -> Messaging {
        let mut m = Messaging::default();
        m.live_warm(&host::projects().unwrap()[0], COMMAND_BUDGET, true);
        m
    }
    use super::*;
    use openidea_plugin::Severity;
    use openidea_plugin::mock::MockHost;

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
        let found: Vec<_> = parse("K.kt", KOTLIN).unwrap().into_iter().map(|e| (e.broker, e.space, e.name, e.role, e.via)).collect();
        assert_eq!(
            found,
            [
                (Kafka, "topic", "orders".into(), Role::Consumer, "@KafkaListener".into()),
                (Kafka, "topic", "audit".into(), Role::Consumer, "@KafkaListener".into()),
                (Kafka, "topic", "replies".into(), Role::Producer, "@SendTo".into()),
                (Rabbit, "queue", format!("{CONSTANT}KtEvents.QUEUE"), Role::Consumer, "@RabbitListener".into()),
                (Jms, "destination", "invoices".into(), Role::Consumer, "@JmsListener".into()),
                (Kafka, "topic", "orders".into(), Role::Producer, "send".into()),
                (Rabbit, "queue", "mails".into(), Role::Producer, "convertAndSend".into()),
            ]
        );
        assert_eq!(names(KAFKA).len(), 5);

        let host = setup();
        host.add_file("src/main/kotlin/com/acme/orders/KtEvents.kt", KOTLIN);
        let mut m = warmed();
        let kt = Document {
            path: "/project/src/main/kotlin/com/acme/orders/KtEvents.kt".into(),
            language: "kotlin".into(),
            version: 1,
            text: KOTLIN.into(),
        };
        let tips: Vec<_> = m.line_markers(&kt).into_iter().map(|x| (x.line, x.tooltip)).collect();
        assert!(tips.iter().any(|(l, t)| *l == 4 && t.contains("OrderEvents.java:8 (send), KtEvents.kt:15 (send)")), "{tips:?}");
        assert!(
            tips.iter().any(|(l, t)| *l == 14 && t.contains("OrderEvents.java:13 (@KafkaListener), KtEvents.kt:5 (@KafkaListener)")),
            "{tips:?}"
        );
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
        assert_eq!(
            found,
            [(Broker::Kafka, "typed-topic".to_owned()), (Broker::Rabbit, "typed-queue".to_owned()), (Broker::Jms, "typed-dest".to_owned()),]
        );
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
        Document { path: format!("/project/src/main/java/com/acme/{path}"), language: "java".into(), version: 1, text: text.into() }
    }

    #[test]
    fn java_provider_cold_cache_returns_fast() {
        let host = setup();
        for n in 0..30 {
            host.add_file(&format!("src/main/java/com/acme/S{n}.java"), &format!("package com.acme;\nclass S{n} {{}}\n"));
        }
        let mut m = Messaging::default();
        let project = host.invocation().project().unwrap();
        let d = doc("orders/OrderEvents.java", KAFKA);
        let started = std::time::Instant::now();
        m.line_markers(&d);
        m.completion(&d, Position::new(5, 10), None);
        m.definition(&d, Position::new(5, 10));
        assert!(started.elapsed() < Duration::from_millis(30));
        let cached = m.index.get(&project, Duration::ZERO).files().len();
        assert!(cached <= 1, "a provider on a cold cache read {cached} files");
    }

    #[test]
    fn links_consumers_and_producers() {
        let _host = setup();
        let mut m = warmed();
        let d = doc("orders/OrderEvents.java", KAFKA);
        let markers = m.line_markers(&d);
        let found: Vec<_> = markers.iter().map(|m| (m.line, m.tooltip.as_str())).collect();
        assert_eq!(
            found,
            [
                (7, "Kafka topic orders: OrderEvents.java:13 (@KafkaListener)"),
                (12, "Kafka topic orders: OrderEvents.java:8 (send), OrderEvents.java:17 (TopicBuilder.name)"),
                (12, "Kafka topic orders: OrderEvents.java:8 (send), OrderEvents.java:17 (TopicBuilder.name)"),
            ]
        );
        assert_eq!(markers[1].target.as_ref().unwrap().range.start, Position::new(7, 23));
        assert_eq!(markers[2].target.as_ref().unwrap().range.start.line, 16, "the TopicBuilder is a second target");
        assert!(markers.iter().all(|m| m.icon.as_deref() == Some(MARKER_ICON)), "same icon: one gutter button per line");

        let rabbit = m.line_markers(&doc("mail/Mailer.java", RABBIT));
        let tips: Vec<_> = rabbit.iter().map(|m| m.line).collect();
        assert_eq!(tips, [7, 8, 13, 13, 16, 16], "producers to mails/events and their consumers, one marker per producer");

        let targets = m.definition(&d, Position::new(16, 61));
        assert_eq!(targets.len(), 2, "the producer and the consumer of `orders`");
    }

    #[test]
    fn completes_known_names_per_broker() {
        let _host = setup();
        let mut m = warmed();
        let typing = KAFKA.replace("streamBridge.send(\"orders-out\", o);", "kafkaTemplate.send(\"o\", o);");
        let items = m.completion(&doc("orders/OrderEvents.java", &typing), Position::new(8, 29), None);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["orders", "audit", "invoices"]);
    }

    #[test]
    fn messaging_view_groups_by_broker_and_destination() {
        let host = setup();
        let mut m = warmed();
        let view = m.render(VIEW, None, &host.invocation()).unwrap();
        let Body::Tree(items) = view.body else { panic!("tree expected") };
        let roots: Vec<_> = items.iter().filter(|i| i.parent.is_none()).map(|i| i.label.as_str()).collect();
        assert_eq!(roots, ["Kafka", "RabbitMQ", "JMS"]);
        let orders: Vec<_> = items.iter().filter(|i| i.parent.as_deref() == Some("d:Kafka:topic:orders")).collect();
        assert_eq!(
            orders.iter().map(|i| i.detail.as_deref().unwrap()).collect::<Vec<_>>(),
            ["consumer · @KafkaListener", "producer · send", "declaration · TopicBuilder.name"]
        );
        assert_eq!(
            orders.iter().map(|i| i.icon.as_deref().unwrap()).collect::<Vec<_>>(),
            ["vcs-incoming", "vcs-outgoing", "spring-config"]
        );
        let destination = |key: &str| {
            let d = items.iter().find(|i| i.key == key).unwrap();
            (d.detail.as_deref().unwrap(), d.icon.as_deref().unwrap())
        };
        assert_eq!(destination("d:Kafka:topic:orders"), ("topic · 1 producer · 1 consumer", "send"));
        assert_eq!(destination("d:Kafka:topic:invoices"), ("topic · 1 producer · 0 consumers", "warning"));
        assert_eq!(view.status.as_deref(), Some("11 destinations"));
        assert!(host.refreshed_views().is_empty(), "the scan is complete");
        m.handle(VIEW, ViewEvent::Activate(orders[1].key.clone()), &host.invocation()).unwrap();
        assert_eq!(host.opened_locations(), [("/project/src/main/java/com/acme/orders/OrderEvents.java".into(), 7, 23)]);
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

    #[cfg(not(feature = "embedded"))]
    #[test]
    fn refresh_resets_incomplete_index() {
        let host = setup();
        let mut m = warmed();
        let project = host.invocation().project().unwrap();
        host.add_file_bytes("src/main/java/com/acme/Legacy.java", b"class caf\xe9 {}");
        m.index.on_events(
            &[openidea_plugin::Event::FileSaved("/project/src/main/java/com/acme/Legacy.java".into())],
            std::time::Instant::now() + COMMAND_BUDGET,
        );
        let index = m.index.get(&project, Duration::ZERO);
        assert!(!index.complete() && !index.scanning());
        let view = m.render(VIEW, None, &host.invocation()).unwrap();
        assert!(view.status.unwrap().contains("partial index (1 unreadable)"));
        m.handle(VIEW, ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
        assert!(m.index.get(&project, Duration::ZERO).files().is_empty(), "an incomplete index starts over");
    }

    #[test]
    fn reads_simple_spel_destinations() {
        assert_eq!(spel_destination("#{'orders'}").as_deref(), Some("orders"));
        assert_eq!(spel_destination("#{'a' + \"-b\"}").as_deref(), Some("a-b"));
        assert_eq!(spel_destination("#{'${app.topic}'}").as_deref(), Some("${app.topic}"));
        assert_eq!(spel_destination("#{T(com.acme.Queues).ORDERS}"), Some(format!("{CONSTANT}Queues.ORDERS")));
        for dynamic in
            ["#{@names.next()}", "#{'a'.concat(x)}", "#{T(Q).A + 'x'}", "#{x}", "pre#{'a'}", "#{'a' + b}", "#{}", "#{cond ? 'a' : 'b'}"]
        {
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
        let mut m = warmed();
        let l = Document { path: "/project/src/main/java/L.java".into(), language: "java".into(), version: 1, text: listeners.into() };
        let lines: Vec<_> = m.line_markers(&l).iter().map(|k| (k.line, k.tooltip.clone())).collect();
        assert_eq!(
            lines,
            [(1, "Kafka topic events: P.java:3 (send)".to_owned()), (2, "RabbitMQ queue orders.in: P.java:4 (convertAndSend)".to_owned()),]
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
        host.add_file("src/main/resources/application-dev.properties", "app.dest=dev-audit\n");
        host.add_file(
            "src/main/resources/application.yml",
            "app:\n  topicName: orders\n  dest: audit\n  reply: invoices\n  other: other-topic\n",
        );
        let mut m = warmed();
        let l = Document { path: "/project/src/main/java/L.java".into(), language: "java".into(), version: 1, text: listeners.into() };
        let lines: Vec<_> = m.line_markers(&l).iter().map(|k| (k.line, k.tooltip.clone())).collect();
        assert_eq!(
            lines,
            [
                (1, "Kafka topic orders: P.java:3 (send)".to_owned()),
                (2, "RabbitMQ queue fallback-q: P.java:4 (convertAndSend)".to_owned()),
                                (3, "JMS destination invoices: P.java:9 (@JmsListener)".to_owned()),
                (5, "Kafka topic other-topic: P.java:6 (send)".to_owned()),
            ],
            "the unresolvable `${{app.nope}}` has no marker; `app.dest` is defined twice with different values"
        );
        let p = Document { path: "/project/src/main/java/P.java".into(), language: "java".into(), version: 1, text: producers.into() };
        let reply: Vec<_> = m.line_markers(&p).iter().map(|k| k.line).collect();
        assert_eq!(reply, [2, 3, 5, 8], "producers to resolved consumers; `invoices` meets the resolved @SendTo");
    }

    #[test]
    fn binding_builder_chain_links_queue_exchange_and_routing_key() {
        let host = MockHost::install();
        let config = r#"class C {
    @Bean Queue orders() { return new Queue("orders.in"); }
    @Bean TopicExchange events() { return new TopicExchange("events"); }
    @Bean Binding binding(Queue orders, TopicExchange events) {
        return BindingBuilder.bind(orders).to(events).with("order.#");
    }
    @Bean Binding dynamic() {
        return BindingBuilder.bind(unknown()).to(events()).with("a");
    }
}
"#;
        host.add_file("src/main/java/C.java", config);
        let found = parse("C.java", config).unwrap();
        let chain: Vec<_> = found.iter().filter(|e| e.via.starts_with("BindingBuilder")).map(|e| (e.space, e.name.as_str(), e.reference)).collect();
        assert_eq!(chain, [("queue", "orders.in", true), ("exchange", "events", true), ("routing key", "events/order.#", false)], "the unresolvable chain stays quiet");

        let mut m = warmed();
        let doc = Document { path: "/project/src/main/java/C.java".into(), language: "java".into(), version: 1, text: config.into() };
        let at = config.find("bind(orders)").unwrap() + 5;
        let targets: Vec<_> = m.definition(&doc, openidea_plugin::text::position(config, at)).iter().map(|l| l.range.start.line).collect();
        assert_eq!(targets, [1], "bind(orders) navigates to new Queue(\"orders.in\")");
        assert!(m.completion(&doc, openidea_plugin::text::position(config, at), None).is_empty(), "no name completion on a bean reference");
    }

    const AMQP_CONFIG: &str = r#"class C {
    @Bean Queue orders() { return new Queue("orders.in"); }
    @Bean Queue all() { return new Queue("all.in"); }
    @Bean TopicExchange events() { return new TopicExchange("events"); }
    @Bean FanoutExchange fan() { return new FanoutExchange("fan"); }
    @Bean HeadersExchange hdr() { return new HeadersExchange("hdr"); }
    @Bean Binding a(Queue orders, TopicExchange events) { return BindingBuilder.bind(orders).to(events).with("order.#"); }
    @Bean Binding b(Queue all, TopicExchange events) { return BindingBuilder.bind(all).to(events).with("order.created"); }
    @Bean Binding f(Queue all, FanoutExchange fan) { return BindingBuilder.bind(all).to(fan); }
    @Bean Binding h(Queue all, HeadersExchange hdr) { return BindingBuilder.bind(all).to(hdr).where("k").exists(); }
}
"#;

    fn producer(imports: &str, decl: &str, call: &str) -> String {
        format!("{imports}\nclass P {{\n    {decl}\n    void go() {{ {call} }}\n}}\n")
    }

    const RT_IMPORT: &str = "import org.springframework.amqp.rabbit.core.RabbitTemplate;";

    fn keys(text: &str) -> Vec<(String, Role)> {
        parse("P.java", text).unwrap_or_default().into_iter().filter(|e| e.space == ROUTING_KEY).map(|e| (e.name, e.role)).collect()
    }

    #[test]
    fn binding_builder_fanout_headers_and_enum_keys_have_no_literal_key() {
        let found = parse("C.java", AMQP_CONFIG).unwrap();
        let keys: Vec<_> = found.iter().filter(|e| e.space == ROUTING_KEY).map(|e| e.name.as_str()).collect();
        assert_eq!(keys, ["events/order.#", "events/order.created"], "fanout and headers bindings carry no routing key");
        let enum_key = "class C { @Bean Queue q() { return new Queue(\"q\"); } @Bean DirectExchange x() { return new DirectExchange(\"x\"); }\n @Bean Binding b() { return BindingBuilder.bind(q).to(x).with(Keys.A); } }";
        let found = parse("C.java", enum_key).unwrap();
        assert!(found.iter().all(|e| e.space != ROUTING_KEY) && found.iter().any(|e| e.space == UNRESOLVED_BINDING));
    }

    #[test]
    fn producer_literals_need_a_proved_amqp_template() {
        let call = r#"rabbitTemplate.convertAndSend("events", "order.created", msg);"#;
        let ok = producer(RT_IMPORT, "private RabbitTemplate rabbitTemplate;", call);
        assert_eq!(keys(&ok), [("events/order.created".to_owned(), Role::Producer)]);
        let amqp = producer("import org.springframework.amqp.core.*;", "AmqpTemplate rabbitTemplate;", call);
        assert_eq!(keys(&amqp).len(), 1, "wildcard import of the exact package");
        let msg = producer("import org.springframework.amqp.rabbit.core.RabbitMessagingTemplate;", "RabbitMessagingTemplate rabbitTemplate;", call);
        assert_eq!(keys(&msg).len(), 1);
        let headers = producer("import org.springframework.amqp.rabbit.core.RabbitMessagingTemplate;", "RabbitMessagingTemplate rabbitTemplate;", r#"rabbitTemplate.convertAndSend("events", "payload", Map.of());"#);
        assert!(keys(&headers).is_empty(), "(destination, payload, headers) is not a routing key");
        assert!(keys(&producer("", "private RabbitTemplate rabbitTemplate;", call)).is_empty(), "no import");
        assert!(keys(&producer("import com.acme.RabbitTemplate;", "RabbitTemplate rabbitTemplate;", call)).is_empty(), "foreign type");
        assert!(keys(&producer(RT_IMPORT, "", call)).is_empty(), "no same-file declaration");
        assert!(keys(&producer(RT_IMPORT, "Foo rabbitTemplate;", call)).is_empty());
        assert!(keys(&producer(RT_IMPORT, "RabbitTemplate rabbitTemplate;", r#"rabbitTemplate.convertAndSend("events", key, msg);"#)).is_empty(), "dynamic key");
        assert!(keys(&producer(RT_IMPORT, "RabbitTemplate rabbitTemplate;", r#"rabbitTemplate.convertAndSend("order.created", msg, m -> m);"#)).is_empty(), "key, payload, post processor");
        assert!(keys(&producer(RT_IMPORT, "RabbitTemplate rabbitTemplate;", r#"rabbitTemplate.convertAndSend("", "q", msg);"#)).is_empty(), "default exchange");
        let send = producer(RT_IMPORT, "RabbitTemplate rabbitTemplate;", r#"rabbitTemplate.send("events", "order.created", message);"#);
        assert_eq!(keys(&send).len(), 1, "send(exchange, key, Message)");
    }

    #[test]
    fn producer_key_navigates_to_every_matching_binding() {
        let host = MockHost::install();
        host.add_file("src/main/java/C.java", AMQP_CONFIG);
        let call = r#"rabbitTemplate.convertAndSend("events", "order.created", msg);"#;
        let text = producer(RT_IMPORT, "RabbitTemplate rabbitTemplate;", call);
        host.add_file("src/main/java/P.java", &text);
        let mut m = warmed();
        let doc = Document { path: "/project/src/main/java/P.java".into(), language: "java".into(), version: 1, text: text.clone() };
        let at = text.find("order.created").unwrap() + 2;
        let mut lines: Vec<_> = m.definition(&doc, openidea_plugin::text::position(&text, at)).iter().map(|l| (l.path.clone(), l.range.start.line)).collect();
        lines.sort();
        assert_eq!(lines, [("/project/src/main/java/C.java".to_owned(), 6), ("/project/src/main/java/C.java".to_owned(), 7)], "the topic pattern and the literal binding, with their files");
        let other = text.replace("order.created", "invoice.paid");
        let doc = Document { text: other.clone(), ..doc };
        assert!(m.definition(&doc, openidea_plugin::text::position(&other, other.find("invoice.paid").unwrap() + 2)).is_empty());
    }

    #[test]
    fn topic_patterns_follow_amqp_wildcards() {
        assert!(topic_matches("order.#", "order.created.eu") && topic_matches("order.#", "order"));
        assert!(topic_matches("*.created", "order.created") && !topic_matches("*.created", "a.b.created"));
        assert!(!topic_matches("order.created", "order.updated") && topic_matches("#", "x.y"));
        assert!(topic_matches("a.#", "a") && topic_matches("a.#", "a.b.c") && !topic_matches("a.#", "b.a"));
        assert!(topic_matches("#.b", "b") && topic_matches("#.b", "x.y.b") && !topic_matches("#.b", "b.x"));
        assert!(topic_matches("a.*.c", "a.b.c") && !topic_matches("a.*.c", "a.c") && !topic_matches("a.*.c", "a.b.b.c"));
    }

    #[test]
    fn topic_matches_many_hashes_fast() {
        let pattern = format!("{}x", "#.".repeat(40));
        let key = format!("{}y", "a.".repeat(60));
        let started = std::time::Instant::now();
        assert!(!topic_matches(&pattern, &key));
        assert!(topic_matches(&pattern, &format!("{key}.x")));
        assert!(started.elapsed() < Duration::from_millis(50));
    }

    #[test]
    fn direct_exchange_binding_is_literal() {
        let config = AMQP_CONFIG.replace("TopicExchange events() { return new TopicExchange", "DirectExchange events() { return new DirectExchange").replace("TopicExchange events)", "DirectExchange events)");
        let found = parse("C.java", &config).unwrap();
        let binding = found.iter().find(|e| e.space == ROUTING_KEY && e.name == "events/order.#").unwrap();
        let sent = |key: &str| Endpoint { role: Role::Producer, name: format!("events/{key}"), via: "send".into(), ..binding.clone() };
        assert!(!binding.binds_key_of(&sent("order.created")));
        assert!(binding.binds_key_of(&sent("order.#")));
        let topic = parse("C.java", AMQP_CONFIG).unwrap();
        let binding = topic.iter().find(|e| e.space == ROUTING_KEY && e.name == "events/order.#").unwrap();
        assert!(binding.binds_key_of(&sent("order.created")));
    }

    #[test]
    fn unbound_key_on_a_declared_direct_or_topic_exchange_is_info() {
        let host = MockHost::install();
        host.add_file("src/main/java/C.java", AMQP_CONFIG);
        let producers = |key: &str, x: &str| producer(RT_IMPORT, "RabbitTemplate rabbitTemplate;", &format!(r#"rabbitTemplate.convertAndSend("{x}", "{key}", msg);"#));
        let text = [producers("invoice.paid", "events"), producers("order.created", "events"), producers("k", "fan"), producers("k", "hdr"), producers("k", "elsewhere")];
        let mut m = warmed();
        let project = host.invocation().project().unwrap();
        for _ in 0..4 {
            m.live_warm(&project, Duration::from_secs(5), true);
        }
        let mut messages = Vec::new();
        for (i, t) in text.iter().enumerate() {
            host.add_file(&format!("src/main/java/P{i}.java"), t);
            let doc = Document { path: format!("/project/src/main/java/P{i}.java"), language: "java".into(), version: 1, text: t.clone() };
            messages.push(m.live_diagnostics(&doc, true).into_iter().map(|d| d.message).collect::<Vec<_>>());
        }
        assert_eq!(messages[0], ["No binding with routing key \"invoice.paid\" for exchange \"events\" found in this project."]);
        assert!(messages[1].is_empty(), "bound by the order.# pattern");
        assert!(messages[2].is_empty() && messages[3].is_empty(), "fanout and headers ignore the key");
        assert!(messages[4].is_empty(), "exchange not declared in the project");
        let doc = Document { path: "/project/src/main/java/P0.java".into(), language: "java".into(), version: 1, text: text[0].clone() };
        assert!(m.live_diagnostics(&doc, false).is_empty(), "no absence claim from a partial index");
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
        let mut m = warmed();
        let x = Document { path: "/project/src/main/resources/beans.xml".into(), language: "xml".into(), version: 1, text: xml.into() };
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
        let mut m = warmed();
        let d = doc("mail/Sender.java", CONSTANT_USER);
        let all: Vec<_> = m.endpoints(&d).into_iter().map(|(p, e)| (file_name(&p).to_owned(), e.name, e.role)).collect();
        assert!(all.contains(&("Sender.java".into(), "mails".into(), Role::Consumer)), "{all:?}");
        assert!(all.contains(&("Sender.java".into(), "mail-events".into(), Role::Producer)));
        assert!(
            !all.iter().any(|(_, n, _)| n.contains("GHOST") || n.contains("UNKNOWN") || n.contains(CONSTANT)),
            "unresolved constants are dropped"
        );
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
        let mut m = warmed();
        m.line_markers(&xml_doc(LISTENERS_XML));
        let found: Vec<_> = host.diagnostics()["/project/src/main/resources/listeners.xml"]
            .iter()
            .map(|d| (d.range.start.line, d.code.clone().unwrap(), d.severity))
            .collect();
        assert_eq!(
            found,
            [
                (11, "missing-attribute".to_owned(), Severity::Warning),
                (13, "missing-attribute".into(), Severity::Warning),
                (13, "invalid-attribute-value".into(), Severity::Warning),
                (4, "invalid-attribute-value".into(), Severity::Warning),
                (5, "missing-attribute".into(), Severity::Warning),
                (6, "unresolved-reference".into(), Severity::Info),
                (5, "unresolved-listener-method".into(), Severity::Warning),
            ]
        );
    }

    #[test]
    fn xml_rabbit_booleans_are_checked() {
        let _host = listener_host();
        let xml = "<beans xmlns:rabbit=\"http://www.springframework.org/schema/rabbit\">\n<rabbit:connection-factory id=\"cf\" publisher-returns=\"yes\"/>\n<rabbit:admin auto-startup=\"true\"/>\n</beans>";
        let found = listener_xml::problems(xml, &spring_text::index::Index::new(&[], facts));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code.as_deref(), Some("invalid-attribute-value"));
    }

    #[test]
    fn xml_listener_ref_and_method_navigate_and_complete() {
        let _host = listener_host();
        let mut m = warmed();
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
        warmed().line_markers(&xml_doc(ok));
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
        assert!(applies(project), "unsure without a readable build file");
        host.add_file("pom.xml", "<project><artifactId>commons-lang3</artifactId></project>");
        assert!(!applies(project));
        host.add_file("pom.xml", "<project><artifactId>spring-kafka</artifactId></project>");
        assert!(applies(project));
        host.add_file("pom.xml", "<project><modules><module>app</module></modules></project>");
        assert!(applies(project), "multi-module is unsure");
    }
}

impl Messaging {
    pub fn live_warm(&mut self, project: &openidea_plugin::Project, budget: std::time::Duration, discover: bool) {
        if budget.is_zero() {
            return;
        }
        self.index.warm(project, budget / 2, discover);
        self.config.warm(project, budget / 2, discover);
    }
    pub fn live_events(&mut self, events: &[openidea_plugin::Event], deadline: std::time::Instant) {
        self.index.on_events(events, deadline);
        self.config.on_events(events, deadline);
    }
}

mod live;
