//! Spring Messaging Tools for OpenIDEA: links the consumers, producers and
//! declarations of Kafka topics, RabbitMQ queues and exchanges and JMS
//! destinations in a Java project by destination name. Text-based.

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Error, Invocation, Location, Plugin, Position, Range, host, register_plugin};
use spring_text::index::{COMMAND_BUDGET, Indexes, PROVIDER_BUDGET};
use spring_text::java::{self, Annotation, Arg, Call, CallArg, JavaFile, Str, Tok};
use spring_text::range;
use spring_text::tree::Expansion;
use std::collections::{BTreeMap, BTreeSet};
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
fn rabbit_target(c: &Call) -> Option<(&'static str, &Str)> {
    let literal = |i: usize| c.args.get(i).and_then(|a| a.string.as_ref());
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

struct Collector<'a> {
    text: &'a str,
    out: Vec<Endpoint>,
}

impl Collector<'_> {
    fn add(&mut self, broker: Broker, space: &'static str, role: Role, via: &str, s: &Str) {
        let v = &s.value;
        if v.is_empty() || v.contains("${") || v.contains("#{") || v.starts_with("!{") {
            return;
        }
        let range = range(self.text, s.at, s.end);
        self.out.push(Endpoint {
            broker,
            space,
            name: v.clone(),
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
        let literal = |i: usize| c.args.get(i).and_then(|a| a.string.as_ref());
        let Some(first) = literal(0) else { return };
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
            return self.add(broker, space, role, &format!("new {}", c.name), first);
        }
        let Some(receiver) = c.receiver.as_deref() else { return };
        let via = format!("{receiver}.{}", c.name);
        match (receiver, c.name.as_str()) {
            ("TopicBuilder", "name") => self.add(Broker::Kafka, "topic", Role::Declaration, &via, first),
            ("QueueBuilder", "durable" | "nonDurable") => {
                self.add(Broker::Rabbit, "queue", Role::Declaration, &via, first)
            }
            ("ExchangeBuilder", "topicExchange" | "directExchange" | "fanoutExchange" | "headersExchange") => {
                self.add(Broker::Rabbit, "exchange", Role::Declaration, &via, first)
            }
            (_, "send" | "convertAndSend" | "convertSendAndReceive" | "sendAndReceive") => {
                match Broker::hinted_by(receiver).or_else(|| declared_type(file, receiver).and_then(Broker::hinted_by))
                {
                    Some(Broker::Kafka) if c.name == "send" => {
                        self.add(Broker::Kafka, "topic", Role::Producer, &c.name, first)
                    }
                    Some(Broker::Rabbit) => {
                        if let Some((space, s)) = rabbit_target(c) {
                            self.add(Broker::Rabbit, space, Role::Producer, &c.name, s);
                        }
                    }
                    Some(Broker::Jms) if c.args.len() >= 2 => {
                        self.add(Broker::Jms, "destination", Role::Producer, &c.name, first)
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

pub fn parse(_path: &str, text: &str) -> Option<Vec<Endpoint>> {
    const HINTS: [&str; 7] = ["Listener", "send", "Send", "Topic", "Queue", "Exchange", "ProducerRecord"];
    if !HINTS.iter().any(|h| text.contains(h)) {
        return None;
    }
    let file = java::parse(text);
    let mut collector = Collector { text, out: Vec::new() };
    for t in &file.types {
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
    for c in java::calls(text) {
        collector.call(&c, &file);
    }
    let mut out = collector.out;
    out.sort_by_key(|e| e.at);
    (!out.is_empty()).then_some(out)
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
    expansion: Expansion,
}

impl Default for Messaging {
    fn default() -> Self {
        Self { index: Indexes::new(&["**/*.java"], parse), expansion: Expansion::default() }
    }
}

impl Messaging {
    /// Every endpoint of the document's project, by file.
    fn endpoints(&mut self, doc: &Document) -> Vec<(String, Endpoint)> {
        let Some(index) = self.index.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else { return Vec::new() };
        index.iter().flat_map(|(p, es)| es.iter().map(move |e| (p.to_owned(), e.clone()))).collect()
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

impl Providers for Messaging {
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
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
        links(doc, &all)
            .into_iter()
            .filter(|(e, _)| e.holds(at))
            .flat_map(|(_, others)| others.into_iter().map(|(p, o)| location(p, o)))
            .collect()
    }

    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let all = self.endpoints(doc);
        let Some(here) = all.iter().find(|(p, e)| *p == doc.path && e.holds(at)).map(|(_, e)| e) else {
            return Vec::new();
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
        let index = self.index.get(&project, COMMAND_BUDGET);
        let complete = index.complete();
        if !complete {
            host::refresh_view(view);
        }
        let refresh = Button::new("refresh", "Refresh").icon("refresh");
        let mut tree: BTreeMap<Broker, Destinations> = BTreeMap::new();
        for (path, endpoints) in index.iter() {
            for e in endpoints {
                tree.entry(e.broker).or_default().entry((e.name.as_str(), e.space)).or_default().push((path, e));
            }
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
            host::refresh_view(view);
            return Ok(());
        }

        match event {
            ViewEvent::Button(b) if b == "refresh" => {
                // Resetting a scan in progress would throw away what it has read.
                if let Some(project) = ctx.project()
                    && self.index.get(&project, Duration::ZERO).complete()
                {
                    self.index.reset();
                }
                host::refresh_view(view);
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

register_plugin!(Messaging, providers, views);

#[cfg(test)]
mod tests {
    use super::*;
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
        parse("", text).unwrap().into_iter().map(|e| (e.broker, e.space, e.name, e.role, e.via)).collect()
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
}
