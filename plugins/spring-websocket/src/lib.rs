//! Spring WebSocket Tools for OpenIDEA: STOMP destinations of a Java
//! project — the broker configuration, `@MessageMapping`/`@SubscribeMapping`
//! handlers and the places sending to a destination — linked together by
//! destination name. Text-based.

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{
    Diagnostic, Error, Invocation, Location, Plugin, Position, Range, Severity, host, register_plugin,
};
use spring_text::index::{Indexes, PROVIDER_BUDGET, project_of};
use spring_text::{parse_source, range, source_calls};
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
    /// `client.subscribe('/topic/x', …)` in a JavaScript STOMP client.
    ClientSubscribe,
    /// `client.publish({ destination })` or `client.send('/app/x')` in a JavaScript STOMP client.
    ClientSend,
}

impl Kind {
    fn is_client(self) -> bool {
        matches!(self, Kind::ClientSubscribe | Kind::ClientSend)
    }

    /// Whether a client use of this kind talks to `other`.
    fn faces(self, other: Kind) -> bool {
        match self {
            Kind::ClientSubscribe => matches!(other, Kind::Send | Kind::SendToUser | Kind::Subscribe),
            Kind::ClientSend => other == Kind::Message,
            Kind::Send | Kind::SendToUser | Kind::Subscribe => other == Kind::ClientSubscribe || !other.is_client(),
            Kind::Message => other == Kind::ClientSend || !other.is_client(),
        }
    }
}

const SCRIPT_EXTENSIONS: [&str; 5] = [".js", ".jsx", ".ts", ".tsx", ".vue"];
const SKIPPED_DIRECTORIES: [&str; 3] = ["/node_modules/", "/dist/", "/build/"];
const CLIENT_FACTORIES: [&str; 6] = ["new Client(", "new Client<", "new StompJs.Client(", "new StompJs.Client<", "Stomp.over(", "Stomp.client("];

fn ident_before(text: &str, end: usize) -> &str {
    let head = &text[..end];
    let start = head
        .rfind(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .map_or(0, |i| i + head[i..].chars().next().map_or(1, char::len_utf8));
    &head[start..]
}

/// Names assigned a STOMP client (`const c = new Client(`, `this.c = Stomp.over(`).
fn client_names(text: &str) -> BTreeSet<&str> {
    let mut names = BTreeSet::new();
    for factory in CLIENT_FACTORIES {
        for (at, _) in text.match_indices(factory) {
            let before = text[..at].trim_end();
            if let Some(before) = before.strip_suffix('=') {
                let before = before.trim_end();
                let statement = before.rsplit(['\n', ';', '{', '}']).next().unwrap_or(before);
                let declared = statement.split(':').next().unwrap_or(statement).trim_end();
                names.insert(ident_before(declared, declared.len()));
            }
        }
    }
    names
}

/// The contents of a plain quoted literal starting at `i`, when it ends the argument.
fn literal_at(text: &str, i: usize) -> Option<(usize, usize)> {
    let quote = *text.as_bytes().get(i)?;
    if !matches!(quote, b'\'' | b'"' | b'`') {
        return None;
    }
    let len = text[i + 1..].find(quote as char)?;
    let (at, end) = (i + 1, i + 1 + len);
    let body = &text[at..end];
    if body.contains(['\n', '\\']) || (quote == b'`' && body.contains("${")) {
        return None;
    }
    let next = text[end + 1..].trim_start().chars().next()?;
    (body.starts_with('/') && matches!(next, ',' | ')' | '}')).then_some((at, end))
}

fn parse_script(text: &str) -> Option<WsFile> {
    if !["subscribe(", "publish(", "send("].iter().any(|h| text.contains(h)) {
        return None;
    }
    let declared = client_names(text);
    let mut out = WsFile::default();
    for (name, kind) in [("subscribe", Kind::ClientSubscribe), ("publish", Kind::ClientSend), ("send", Kind::ClientSend)] {
        let pattern = format!(".{name}(");
        for (dot, _) in text.match_indices(&pattern) {
            let receiver = ident_before(text, dot);
            let lower = receiver.to_lowercase();
            if !(lower.contains("stomp") || declared.contains(receiver)) {
                continue;
            }
            let arg = dot + pattern.len();
            let arg = arg + (text[arg..].len() - text[arg..].trim_start().len());
            let literal = if name == "publish" {
                let object = text[arg..].strip_prefix('{').and_then(|_| text[arg..].find('}'));
                object
                    .and_then(|close| text[arg..arg + close].find("destination").map(|k| arg + k + "destination".len()))
                    .and_then(|k| {
                        let rest = text[k..].trim_start().strip_prefix(':')?;
                        let value = text.len() - rest.trim_start().len();
                        literal_at(text, value)
                    })
            } else {
                literal_at(text, arg)
            };
            let Some((at, end)) = literal else { continue };
            let via = format!("{receiver}.{name}");
            out.uses.push(Use { kind, dest: text[at..end].to_owned(), via, at, end, range: range(text, at, end) });
        }
    }
    out.uses.sort_by_key(|u| u.at);
    (!out.uses.is_empty()).then_some(out)
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

pub fn parse(path: &str, text: &str) -> Option<WsFile> {
    if SCRIPT_EXTENSIONS.iter().any(|e| path.ends_with(e)) {
        let normalized = path.replace('\\', "/");
        if SKIPPED_DIRECTORIES.iter().any(|d| normalized.contains(d)) {
            return None;
        }
        return parse_script(text);
    }
    const HINTS: [&str; 6] = ["Mapping", "SendTo", "convertAndSend", ".send", "Broker", "addEndpoint"];
    if !HINTS.iter().any(|h| text.contains(h)) {
        return None;
    }
    let file = parse_source(path, text);
    let mut out = WsFile::default();
    let push = |out: &mut WsFile, kind, dest: String, via: &str, s: &spring_text::java::Str| {
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
    for c in source_calls(path, text) {
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
            Kind::Send | Kind::ClientSubscribe | Kind::ClientSend => vec![u.dest.clone()],
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
            .filter(|(p, o)| !(p == path && o.at == u.at) && u.kind.faces(o.kind) && self.config.keys(o).iter().any(|k| keys.contains(k)))
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

const BUILD_FILES: [&str; 3] = ["pom.xml", "build.gradle", "build.gradle.kts"];
const STOMP_DEPENDENCIES: [&str; 3] = ["spring-boot-starter-websocket", "spring-messaging", "spring-websocket"];

fn is_script(path: &str) -> bool {
    SCRIPT_EXTENSIONS.iter().any(|e| path.ends_with(e))
}

pub struct WebSocketTools {
    index: Indexes<WsFile>,
    /// Project roots known to serve STOMP: a build file names the dependency, or a Java/Kotlin file was opened.
    stomp_roots: BTreeSet<String>,
}

impl Default for WebSocketTools {
    fn default() -> Self {
        Self {
            index: Indexes::new(
                &["**/*.java", "**/*.kt", "**/*.js", "**/*.jsx", "**/*.ts", "**/*.tsx", "**/*.vue"],
                parse,
            ),
            stomp_roots: BTreeSet::new(),
        }
    }
}

impl WebSocketTools {
    /// Script documents are only indexed in projects whose root build file names a Spring messaging dependency,
    /// or after a Java/Kotlin document of the project was opened (multi-module layouts).
    fn serves_stomp(&mut self, doc: &Document) -> bool {
        let Some(project) = project_of(&doc.path) else { return false };
        let Ok(root) = project.root() else { return false };

        if self.stomp_roots.contains(&root) {
            return true;
        }

        let names_dependency = BUILD_FILES.iter().any(|file| {
            project
                .read_file(file)
                .is_ok_and(|text| STOMP_DEPENDENCIES.iter().any(|dependency| text.contains(dependency)))
        });

        if names_dependency {
            self.stomp_roots.insert(root);
        }

        names_dependency
    }

    fn snapshot(&mut self, doc: &Document) -> Snapshot {
        let mut config = Config::default();
        let mut uses = Vec::new();
        let mut user = None;

        if is_script(&doc.path) && !self.serves_stomp(doc) {
            config.user = DEFAULT_USER_PREFIX.to_owned();
            return Snapshot { config, uses };
        }

        if !is_script(&doc.path)
            && let Some(root) = project_of(&doc.path).and_then(|p| p.root().ok())
        {
            self.stomp_roots.insert(root);
        }

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
                    let mut tooltip = keys.join(", ");
                    if !related.is_empty() {
                        let list: Vec<String> = related.iter().map(|r| describe(r)).collect();
                        tooltip = format!("{tooltip}; also used at {}", list.join(", "));
                    }
                    let target = related.first().map(|r| location(r));
                    markers.push(LineMarker { line: u.range.start.line, icon: None, tooltip, target });
                }
                Kind::Send | Kind::SendToUser | Kind::ClientSubscribe | Kind::ClientSend => {
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
        host.add_file("pom.xml", "<artifactId>spring-boot-starter-websocket</artifactId>");
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

    const KOTLIN_CHAT: &str = r#"package com.acme.chat

@Controller
@MessageMapping("/kchat")
class KtChatController(private val messagingTemplate: SimpMessagingTemplate) {
    @MessageMapping("/send")
    @SendTo("/topic/ktmessages")
    suspend fun send(m: Message): Message = m

    @SubscribeMapping(value = ["/krooms"])
    fun rooms(): List<Room> = emptyList()

    fun broadcast(m: Message) {
        messagingTemplate.convertAndSend("/topic/ktmessages", m)
        messagingTemplate.convertAndSendToUser("bob", "/queue/replies", m)
        kafkaTemplate.send("orders", m)
    }
}
"#;

    #[test]
    fn kotlin_message_mapping_is_indexed() {
        let chat = parse("KtChat.kt", KOTLIN_CHAT).unwrap();
        let uses: Vec<_> = chat.uses.iter().map(|u| (u.kind, u.dest.as_str(), u.via.as_str())).collect();
        assert_eq!(uses, [
            (Kind::Message, "/kchat/send", "@MessageMapping"),
            (Kind::Send, "/topic/ktmessages", "@SendTo"),
            (Kind::Subscribe, "/kchat/krooms", "@SubscribeMapping"),
            (Kind::Send, "/topic/ktmessages", "convertAndSend"),
            (Kind::SendToUser, "/queue/replies", "convertAndSendToUser"),
        ]);
        assert_eq!(parse("", CHAT).unwrap().uses.len(), 8);

        let host = setup();
        host.add_file("src/main/kotlin/com/acme/chat/KtChatController.kt", KOTLIN_CHAT);
        let mut tools = WebSocketTools::default();
        let path = "/project/src/main/kotlin/com/acme/chat/KtChatController.kt";
        let kt = Document { path: path.into(), language: "kotlin".into(), version: 1, text: KOTLIN_CHAT.into() };
        let tips: Vec<_> = tools.line_markers(&kt).into_iter().map(|m| (m.line, m.tooltip)).collect();
        assert!(tips.iter().any(|(l, t)| *l == 5 && t.starts_with("SEND /app/kchat/send")), "{tips:?}");
        assert!(tips.iter().any(|(l, t)| *l == 13 && t.contains("KtChatController.kt:7 (@SendTo)")), "{tips:?}");
    }

    #[test]
    fn kotlin_template_destinations_are_not_indexed() {
        let src = "class C(val messagingTemplate: SimpMessagingTemplate) {\n    fun f(room: String) {\n        messagingTemplate.convertAndSend(\"/topic/$room\", 1)\n        messagingTemplate.convertAndSend(\"/topic/all\", 1)\n    }\n}\n";
        let dests: Vec<_> = parse("C.kt", src).unwrap().uses.into_iter().map(|u| u.dest).collect();
        assert_eq!(dests, ["/topic/all"]);
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

    const CLIENT: &str = r#"import { Client } from '@stomp/stompjs';
const client = new Client({ brokerURL: 'ws://localhost/ws' });
const emitter = makeEmitter();
client.onConnect = () => {
  client.subscribe('/topic/messages', (m) => show(m));
  client.publish({ destination: '/app/chat/send', body: 'hi' });
  stompClient.send('/app/chat/private', {}, 'x');
  emitter.subscribe('/topic/messages', cb);
  client.subscribe(`/topic/${room}`, cb);
  client.subscribe('/topic/' + room, cb);
  client.publish({ destination: dest, body: '' });
};
"#;

    fn script_doc() -> Document {
        Document {
            path: "/project/web/src/chat.ts".into(),
            language: "typescript".into(),
            version: 1,
            text: CLIENT.into(),
        }
    }

    #[test]
    fn script_indexes_only_literal_stomp_calls() {
        let uses: Vec<_> =
            parse("chat.ts", CLIENT).unwrap().uses.iter().map(|u| (u.kind, u.dest.clone())).collect();
        assert_eq!(
            uses,
            [
                (Kind::ClientSubscribe, "/topic/messages".to_owned()),
                (Kind::ClientSend, "/app/chat/send".to_owned()),
                (Kind::ClientSend, "/app/chat/private".to_owned()),
            ]
        );
        assert!(parse("/p/node_modules/x/chat.js", CLIENT).is_none());
    }

    #[test]
    fn js_documents_skip_indexing_without_a_spring_project() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/chat/ChatController.java", CHAT);
        host.add_file("web/src/chat.ts", CLIENT);
        let mut tools = WebSocketTools::default();
        let d = script_doc();

        assert!(tools.line_markers(&d).is_empty());
        assert!(tools.definition(&d, Position::new(5, 40)).is_empty());
        assert!(tools.completion(&d, Position::new(5, 40), None).is_empty());
    }

    #[test]
    fn js_documents_are_indexed_in_a_spring_project() {
        let host = setup();
        host.add_file("web/src/chat.ts", CLIENT);
        let mut tools = WebSocketTools::default();

        assert!(!tools.line_markers(&script_doc()).is_empty());
    }

    #[test]
    fn js_subscribe_links_to_send_to_destination() {
        let host = setup();
        host.add_file("web/src/chat.ts", CLIENT);
        let mut tools = WebSocketTools::default();
        let d = script_doc();
        let targets = tools.definition(&d, Position::new(4, 24));
        let paths: Vec<_> = targets.iter().map(|t| (t.path.rsplit('/').next().unwrap(), t.range.start.line)).collect();
        assert_eq!(paths, [("ChatController.java", 8), ("ChatController.java", 19)]);

        let java = doc("ChatController.java", CHAT);
        let markers = tools.line_markers(&java);
        assert!(markers[1].tooltip.contains("chat.ts:5 (client.subscribe)"), "{}", markers[1].tooltip);
        let marker = tools.line_markers(&d);
        assert_eq!(marker[0].line, 4);
        assert_eq!(marker[0].target.as_ref().unwrap().range.start.line, 8);
    }

    #[test]
    fn js_send_links_to_message_mapping() {
        let host = setup();
        host.add_file("web/src/chat.ts", CLIENT);
        let mut tools = WebSocketTools::default();
        let d = script_doc();
        let targets = tools.definition(&d, Position::new(5, 40));
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].range.start.line, 7);
        assert_eq!(tools.definition(&d, Position::new(6, 25)).len(), 1);

        let markers = tools.line_markers(&doc("ChatController.java", CHAT));
        assert!(markers[0].tooltip.starts_with("SEND /app/chat/send; also used at chat.ts:6"), "{}", markers[0].tooltip);
        assert!(markers[0].target.as_ref().unwrap().path.ends_with("chat.ts"));
    }

    #[test]
    fn js_quiet_for_emitters_and_runtime_destinations() {
        let host = setup();
        host.add_file("web/src/chat.ts", CLIENT);
        let mut tools = WebSocketTools::default();
        let d = script_doc();
        assert!(tools.definition(&d, Position::new(7, 24)).is_empty());
        assert!(tools.definition(&d, Position::new(8, 24)).is_empty());
        assert!(tools.line_markers(&d).iter().all(|m| m.line < 7));
    }

    #[test]
    fn non_stomp_client_receiver_is_not_linked() {
        let host = setup();
        let text = "const api = makeApi();\napiClient.publish({ destination: '/chat/send' });\nhttpClient.send('/chat/send', body);\n";
        host.add_file("web/src/api.ts", text);
        assert!(parse("api.ts", text).is_none());
        let declared = format!("const client = new StompJs.Client({{}});\n{text}client.send('/chat/send', {{}});\n");
        assert_eq!(parse("api.ts", &declared).unwrap().uses.len(), 1);
        let mut tools = WebSocketTools::default();
        let d = Document { path: "/project/web/src/api.ts".into(), language: "typescript".into(), version: 1, text: text.into() };
        assert!(tools.line_markers(&d).is_empty());
        assert!(tools.definition(&d, Position::new(2, 20)).is_empty());
    }

    #[test]
    fn typed_declaration_receiver_is_linked() {
        let _host = setup();
        let text = "class Chat {\n  private stomp2: StompJs.Client = new StompJs.Client({});\n  connect() {\n    this.link = new Client<Opts>({});\n    const client: Client = new Client({});\n    client.subscribe('/topic/messages', cb);\n    this.stomp2.send('/app/chat/send', {}, '');\n    this.link.subscribe('/topic/messages', cb);\n  }\n}\n";
        let kinds: Vec<_> = parse("chat.ts", text).unwrap().uses.iter().map(|u| u.kind).collect();
        assert_eq!(kinds, [Kind::ClientSubscribe, Kind::ClientSend, Kind::ClientSubscribe]);
    }
}
