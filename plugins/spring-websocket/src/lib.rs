//! Spring WebSocket Tools for OpenIDEA: STOMP destinations of a Java
//! project — the broker configuration, `@MessageMapping`/`@SubscribeMapping`
//! handlers and the places sending to a destination — linked together by
//! destination name. Text-based.

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{
    Diagnostic, Error, Invocation, Location, Plugin, Position, Range, Severity, host, register_plugin,
};
use spring_text::index::{Indexes, PROVIDER_BUDGET};
use spring_text::java;
use spring_text::range;
use std::collections::BTreeSet;

const DEFAULT_USER_PREFIX: &str = "/user/";
/// Receivers named like a STOMP messaging template.
const STOMP_RECEIVERS: [&str; 4] = ["messaging", "simp", "stomp", "websocket"];
/// Receivers of other brokers' templates (`kafkaTemplate.send`).
const OTHER_RECEIVERS: [&str; 4] = ["kafka", "rabbit", "amqp", "jms"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `@MessageMapping`: a destination clients send to.
    Message,
    /// `@SubscribeMapping`: a destination clients subscribe to, answered by the application.
    Subscribe,
    /// `@SendTo`, `convertAndSend`, `send`: a broker destination.
    Send,
    /// `@SendToUser`, `convertAndSendToUser`: a user destination.
    SendToUser,
}

/// A destination as written in a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Use {
    pub kind: Kind,
    /// With the class-level `@MessageMapping` prefix for handlers.
    pub dest: String,
    /// What names it: `@SendTo`, `convertAndSend`…
    pub via: String,
    /// The bytes and range of the literal's contents.
    pub at: usize,
    pub end: usize,
    pub range: Range,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WsFile {
    pub brokers: Vec<String>,
    pub apps: Vec<String>,
    pub user: Option<String>,
    pub endpoints: Vec<String>,
    pub uses: Vec<Use>,
}

/// A class-level mapping prefix joined to a method's (`/` or `.` separated destinations).
fn join(prefix: &str, path: &str) -> String {
    match (prefix, path) {
        ("", p) | (p, "") => p.to_owned(),
        (a, b) => format!("{}/{}", a.trim_end_matches('/'), b.trim_start_matches('/')),
    }
}

fn is_placeholder(s: &str) -> bool {
    s.contains("${") || s.contains("#{")
}

pub fn parse(_path: &str, text: &str) -> Option<WsFile> {
    const HINTS: [&str; 6] = ["Mapping", "SendTo", "convertAndSend", ".send", "Broker", "addEndpoint"];
    if !HINTS.iter().any(|h| text.contains(h)) {
        return None;
    }
    let file = java::parse(text);
    let mut out = WsFile::default();
    let push = |out: &mut WsFile, kind, dest: String, via: &str, s: &java::Str| {
        if !is_placeholder(&dest) {
            let range = range(text, s.at, s.end);
            out.uses.push(Use { kind, dest, via: via.to_owned(), at: s.at, end: s.end, range });
        }
    };
    for t in &file.types {
        let prefixes: Vec<String> = match t.annotation("MessageMapping").map(|a| a.strings(&["value"])) {
            Some(s) if !s.is_empty() => s.iter().map(|s| s.value.clone()).collect(),
            _ => vec![String::new()],
        };
        for m in t.methods() {
            for (name, kind) in [("MessageMapping", Kind::Message), ("SubscribeMapping", Kind::Subscribe)] {
                for s in m.annotation(name).map(|a| a.strings(&["value"])).unwrap_or_default() {
                    for prefix in &prefixes {
                        push(&mut out, kind, join(prefix, &s.value), &format!("@{name}"), s);
                    }
                }
            }
            if !m.has(&["MessageMapping", "SubscribeMapping"]) {
                continue;
            }
            for (name, kind) in [("SendTo", Kind::Send), ("SendToUser", Kind::SendToUser)] {
                for s in m.annotation(name).map(|a| a.strings(&["value", "destinations"])).unwrap_or_default() {
                    push(&mut out, kind, s.value.clone(), &format!("@{name}"), s);
                }
            }
        }
    }
    let configurer = file.types.iter().any(|t| {
        t.supers().any(|s| s.name.contains("WebSocketMessageBrokerConfigurer"))
            || t.methods().any(|m| m.name == "configureMessageBroker" || m.name == "registerStompEndpoints")
    });
    for c in java::calls(text) {
        let strings = || c.args.iter().filter_map(|a| a.string.as_ref().map(|s| s.value.clone()));
        let receiver = c.receiver.as_deref().unwrap_or("").to_lowercase();
        let literal = |i: usize| c.args.get(i).and_then(|a| a.string.as_ref());
        match c.name.as_str() {
            "enableSimpleBroker" | "enableStompBrokerRelay" if configurer => out.brokers.extend(strings()),
            "setApplicationDestinationPrefixes" if configurer => out.apps.extend(strings()),
            "setUserDestinationPrefix" if configurer => out.user = strings().next(),
            "addEndpoint" if configurer => out.endpoints.extend(strings()),
            "convertAndSend" | "send" if c.args.len() >= 2 => {
                let Some(s) = literal(0) else { continue };
                let stomp = STOMP_RECEIVERS.iter().any(|r| receiver.contains(r))
                    || s.value.starts_with('/') && !OTHER_RECEIVERS.iter().any(|r| receiver.contains(r));
                if stomp {
                    push(&mut out, Kind::Send, s.value.clone(), &c.name, s);
                }
            }
            "convertAndSendToUser" if c.args.len() >= 3 => {
                if let Some(s) = literal(1) {
                    push(&mut out, Kind::SendToUser, s.value.clone(), &c.name, s);
                }
            }
            _ => {}
        }
    }
    (out != WsFile::default()).then_some(out)
}

/// The project's broker configuration, merged from every file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub brokers: Vec<String>,
    pub apps: Vec<String>,
    pub user: String,
}

impl Config {
    /// The destinations a client uses for `u`: application prefix + mapping,
    /// user prefix + user destination.
    fn keys(&self, u: &Use) -> Vec<String> {
        match u.kind {
            Kind::Send => vec![u.dest.clone()],
            Kind::SendToUser => vec![join(&self.user, &u.dest)],
            Kind::Message | Kind::Subscribe => {
                let mut keys: Vec<String> = self.apps.iter().map(|p| join(p, &u.dest)).collect();
                if keys.is_empty() || u.kind == Kind::Subscribe {
                    keys.push(u.dest.clone());
                }
                keys
            }
        }
    }

    /// Whether a broker destination reaches the broker.
    fn routed(&self, dest: &str) -> bool {
        self.brokers.iter().any(|p| dest.starts_with(p.as_str())) || dest.starts_with(&self.user)
    }
}

/// Every destination of the project with its file, and the broker configuration.
struct Snapshot {
    config: Config,
    uses: Vec<(String, Use)>,
}

impl Snapshot {
    fn of_document<'a>(&'a self, doc: &'a Document) -> impl Iterator<Item = &'a Use> {
        self.uses.iter().filter(move |(p, _)| *p == doc.path).map(|(_, u)| u)
    }

    /// The other places using one of `u`'s destinations.
    fn related(&self, path: &str, u: &Use) -> Vec<&(String, Use)> {
        let keys = self.config.keys(u);
        self.uses
            .iter()
            .filter(|(p, o)| !(p == path && o.at == u.at) && self.config.keys(o).iter().any(|k| keys.contains(k)))
            .collect()
    }
}

fn location((path, u): &(String, Use)) -> Location {
    Location { path: path.clone(), range: u.range }
}

fn describe((path, u): &(String, Use)) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    format!("{file}:{} ({})", u.range.start.line + 1, u.via)
}

pub struct WebSocketTools {
    index: Indexes<WsFile>,
}

impl Default for WebSocketTools {
    fn default() -> Self {
        Self { index: Indexes::new(&["**/*.java"], parse) }
    }
}

impl WebSocketTools {
    fn snapshot(&mut self, doc: &Document) -> Snapshot {
        let mut config = Config::default();
        let mut uses = Vec::new();
        let mut user = None;
        if let Some(index) = self.index.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) {
            for (path, file) in index.iter() {
                config.brokers.extend(file.brokers.iter().cloned());
                config.apps.extend(file.apps.iter().cloned());
                user = user.or(file.user.clone());
                uses.extend(file.uses.iter().map(|u| (path.to_owned(), u.clone())));
            }
        }
        config.user = user.map_or(DEFAULT_USER_PREFIX.to_owned(), |u| format!("{}/", u.trim_end_matches('/')));
        Snapshot { config, uses }
    }
}

impl Providers for WebSocketTools {
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let snap = self.snapshot(doc);
        let mut markers = Vec::new();
        let mut problems = Vec::new();
        for u in snap.of_document(doc) {
            let related = snap.related(&doc.path, u);
            match u.kind {
                Kind::Message | Kind::Subscribe => {
                    let verb = if u.kind == Kind::Message { "SEND" } else { "SUBSCRIBE" };
                    let keys: Vec<String> = snap.config.keys(u).iter().map(|k| format!("{verb} {k}")).collect();
                    let target = related.first().map(|r| location(r));
                    markers.push(LineMarker { line: u.range.start.line, icon: None, tooltip: keys.join(", "), target });
                }
                Kind::Send | Kind::SendToUser => {
                    if u.kind == Kind::Send && !snap.config.brokers.is_empty() && !snap.config.routed(&u.dest) {
                        let message = format!(
                            "`{}` is not under a broker prefix ({}), so subscribers won't receive it.",
                            u.dest,
                            snap.config.brokers.join(", ")
                        );
                        problems
                            .push(Diagnostic::new(u.range, Severity::Warning, message).code("unrouted-destination"));
                    }
                    let Some(first) = related.first() else { continue };
                    let list: Vec<String> = related.iter().map(|r| describe(r)).collect();
                    let tooltip = format!("{} is also used at {}.", snap.config.keys(u)[0], list.join(", "));
                    markers.push(LineMarker {
                        line: u.range.start.line,
                        icon: None,
                        tooltip,
                        target: Some(location(first)),
                    });
                }
            }
        }
        let _ = host::publish_diagnostics(&doc.path, &problems);
        markers
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let snap = self.snapshot(doc);
        snap.of_document(doc)
            .filter(|u| u.at <= at && at <= u.end)
            .flat_map(|u| snap.related(&doc.path, u).into_iter().map(location))
            .collect()
    }

    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let snap = self.snapshot(doc);
        let Some(here) = snap.of_document(doc).find(|u| u.at <= at && at <= u.end) else { return Vec::new() };
        let same_side = |k: Kind| match here.kind {
            Kind::Message | Kind::Subscribe => matches!(k, Kind::Message | Kind::Subscribe),
            other => k == other,
        };
        let mut items = Vec::new();
        let mut seen = BTreeSet::new();
        if here.kind == Kind::Send {
            for prefix in &snap.config.brokers {
                let label = format!("{}/", prefix.trim_end_matches('/'));
                if seen.insert(label.clone()) {
                    items.push(CompletionItem {
                        detail: Some("broker prefix".into()),
                        ..CompletionItem::new(&label, CompletionKind::Folder)
                    });
                }
            }
        }
        for (path, u) in &snap.uses {
            if same_side(u.kind) && !(*path == doc.path && u.at == here.at) && seen.insert(u.dest.clone()) {
                items.push(CompletionItem {
                    detail: Some(u.via.clone()),
                    ..CompletionItem::new(&u.dest, CompletionKind::Value)
                });
            }
        }
        let replace = range(&doc.text, here.at, here.end);
        items.into_iter().map(|i| CompletionItem { replace: Some(replace), ..i }).collect()
    }
}

impl Plugin for WebSocketTools {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

register_plugin!(WebSocketTools, providers);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    const CONFIG: &str = r#"package com.acme.chat;

@Configuration
@EnableWebSocketMessageBroker
public class WebSocketConfig implements WebSocketMessageBrokerConfigurer {
    @Override
    public void configureMessageBroker(MessageBrokerRegistry registry) {
        registry.enableSimpleBroker("/topic", "/queue");
        registry.setApplicationDestinationPrefixes("/app");
    }

    @Override
    public void registerStompEndpoints(StompEndpointRegistry registry) {
        registry.addEndpoint("/ws").withSockJS();
    }
}
"#;

    const CHAT: &str = r#"package com.acme.chat;

@Controller
@MessageMapping("/chat")
public class ChatController {
    private final SimpMessagingTemplate messagingTemplate;

    @MessageMapping("/send")
    @SendTo("/topic/messages")
    public Message send(Message m) { return m; }

    @SubscribeMapping("/rooms")
    public List<Room> rooms() { return List.of(); }

    @MessageMapping("/private")
    @SendToUser("/queue/replies")
    public Message reply(Message m) { return m; }

    public void broadcast(Message m) {
        messagingTemplate.convertAndSend("/topic/messages", m);
        messagingTemplate.convertAndSend("/news/today", m);
        messagingTemplate.convertAndSendToUser("bob", "/queue/replies", m);
        kafkaTemplate.send("orders", m);
        messagingTemplate.convertAndSend("${dest}", m);
    }
}
"#;

    fn setup() -> MockHost {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/chat/WebSocketConfig.java", CONFIG);
        host.add_file("src/main/java/com/acme/chat/ChatController.java", CHAT);
        host
    }

    fn doc(name: &str, text: &str) -> Document {
        Document {
            path: format!("/project/src/main/java/com/acme/chat/{name}"),
            language: "java".into(),
            version: 1,
            text: text.into(),
        }
    }

    #[test]
    fn indexes_configuration_handlers_and_senders() {
        let config = parse("", CONFIG).unwrap();
        assert_eq!(
            (config.brokers, config.apps, config.endpoints),
            (vec!["/topic".to_owned(), "/queue".into()], vec!["/app".to_owned()], vec!["/ws".to_owned()])
        );
        let chat = parse("", CHAT).unwrap();
        let uses: Vec<_> = chat.uses.iter().map(|u| (u.kind, u.dest.as_str(), u.via.as_str())).collect();
        assert_eq!(
            uses,
            [
                (Kind::Message, "/chat/send", "@MessageMapping"),
                (Kind::Send, "/topic/messages", "@SendTo"),
                (Kind::Subscribe, "/chat/rooms", "@SubscribeMapping"),
                (Kind::Message, "/chat/private", "@MessageMapping"),
                (Kind::SendToUser, "/queue/replies", "@SendToUser"),
                (Kind::Send, "/topic/messages", "convertAndSend"),
                (Kind::Send, "/news/today", "convertAndSend"),
                (Kind::SendToUser, "/queue/replies", "convertAndSendToUser"),
            ]
        );
        assert!(parse("", "class A { void f() { x.y(); } }").is_none());
    }

    #[test]
    fn marks_handlers_and_links_destinations() {
        let host = setup();
        let mut tools = WebSocketTools::default();
        let d = doc("ChatController.java", CHAT);
        let markers = tools.line_markers(&d);
        let found: Vec<_> = markers.iter().map(|m| (m.line, m.tooltip.as_str())).collect();
        assert_eq!(
            found,
            [
                (7, "SEND /app/chat/send"),
                (8, "/topic/messages is also used at ChatController.java:20 (convertAndSend)."),
                (11, "SUBSCRIBE /app/chat/rooms, SUBSCRIBE /chat/rooms"),
                (14, "SEND /app/chat/private"),
                (15, "/user/queue/replies is also used at ChatController.java:22 (convertAndSendToUser)."),
                (19, "/topic/messages is also used at ChatController.java:9 (@SendTo)."),
                (21, "/user/queue/replies is also used at ChatController.java:16 (@SendToUser)."),
            ]
        );
        assert_eq!(markers[1].target.as_ref().unwrap().range.start, Position::new(19, 42));

        let problems = &host.diagnostics()["/project/src/main/java/com/acme/chat/ChatController.java"];
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].range.start, Position::new(20, 42));
        assert_eq!(problems[0].code.as_deref(), Some("unrouted-destination"));

        let targets = tools.definition(&d, Position::new(19, 45));
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].range.start, Position::new(8, 13));
    }

    #[test]
    fn no_warning_without_broker_configuration() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/chat/ChatController.java", CHAT);
        WebSocketTools::default().line_markers(&doc("ChatController.java", CHAT));
        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn completes_prefixes_and_known_destinations() {
        let _host = setup();
        let mut tools = WebSocketTools::default();
        let typing = CHAT.replace("\"/news/today\"", "\"/t\"");
        let items = tools.completion(&doc("ChatController.java", &typing), Position::new(20, 43), None);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["/topic/", "/queue/", "/topic/messages"]);
        assert_eq!(items[0].replace, Some(Range::new(Position::new(20, 42), Position::new(20, 44))));
    }
}
