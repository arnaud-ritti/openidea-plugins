//! Explicit finite GraphQL subscription sessions through a reviewed Node client.
//! This is a manual document transport, not a runtime resolver/schema model.
use openidea_plugin::{
    Error, Invocation, Project,
    host::{self, Child, Command},
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
const HELPER: &str = include_str!("../runtime/graphql-subscription.cjs");
const OUTPUT: &str = "Spring GraphQL Subscriptions";
pub(crate) const START: &str = "graphql.subscription-start";
pub(crate) const POLL: &str = "graphql.subscription-poll";
pub(crate) const CANCEL: &str = "graphql.subscription-cancel";
/// Read as `plugin.aritti.spring-web.graphqlSubscriptionActive` by the Read/Cancel commands and Tools menu entries.
const ACTIVE_CONTEXT: &str = "graphqlSubscriptionActive";
const NOT_RUNNING: &str = "No GraphQL subscription is running. Start one with Spring Endpoints: Start GraphQL Subscription…";
fn invalid(s: &str) -> Error {
    Error::Invalid(s.into())
}
fn name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_alphabetic() || b == b'_' || i > 0 && b.is_ascii_digit())
}
fn tokens(text: &str) -> Result<Vec<&str>, Error> {
    let bytes = text.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < bytes.len() {
        let start = i;
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' | b',' => {
                i += 1;
                continue;
            }
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1
                }
                continue;
            }
            b'"' => {
                if text[i..].starts_with("\"\"\"") {
                    i += 3;
                    let mut closed = false;
                    while i < bytes.len() {
                        if text[i..].starts_with("\"\"\"") && !text[..i].ends_with('\\') {
                            i += 3;
                            closed = true;
                            break;
                        }
                        i += text[i..].chars().next().unwrap().len_utf8()
                    }
                    if !closed {
                        return Err(invalid("Unclosed GraphQL block string."));
                    }
                } else {
                    i += 1;
                    let mut closed = false;
                    while i < bytes.len() {
                        match bytes[i] {
                            b'"' => {
                                i += 1;
                                closed = true;
                                break;
                            }
                            b'\\' => {
                                i += 1;
                                if i >= bytes.len() {
                                    break;
                                }
                                i += text[i..].chars().next().unwrap().len_utf8()
                            }
                            b'\n' | b'\r' => break,
                            _ => i += text[i..].chars().next().unwrap().len_utf8(),
                        }
                    }
                    if !closed {
                        return Err(invalid("Unclosed GraphQL string."));
                    }
                }
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1
                }
            }
            b'0'..=b'9' | b'-' => {
                i += 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'.' | b'+' | b'-'))
                {
                    i += 1
                }
            }
            b'!' | b'$' | b'(' | b')' | b':' | b'=' | b'@' | b'[' | b']' | b'{' | b'}' | b'|'
            | b'&' | b'.' => i += 1,
            _ => return Err(invalid("Unsupported character outside GraphQL strings.")),
        }
        out.push(&text[start..i]);
        if out.len() > 4096 {
            return Err(invalid("GraphQL document has too many tokens."));
        }
    }
    Ok(out)
}
fn selection_end(tokens: &[&str], mut at: usize) -> Result<usize, Error> {
    let mut stack = Vec::new();
    let mut selection = None;
    while at < tokens.len() {
        let token = tokens[at];
        match token {
            "(" | "[" | "{" => {
                if token == "{" && stack.is_empty() {
                    selection = Some(at)
                }
                stack.push(token);
                if stack.len() > 64 {
                    return Err(invalid("GraphQL nesting exceeds 64 levels."));
                }
            }
            ")" | "]" | "}" => {
                let expected = match token {
                    ")" => "(",
                    "]" => "[",
                    _ => "{",
                };
                if stack.pop() != Some(expected) {
                    return Err(invalid("Unbalanced GraphQL document."));
                }
                if stack.is_empty() && selection.is_some() {
                    if selection == Some(at - 1) {
                        return Err(invalid("Subscription selection cannot be empty."));
                    }
                    return Ok(at + 1);
                }
            }
            _ => {}
        }
        at += 1;
    }
    Err(invalid("GraphQL subscription has no complete selection."))
}
fn subscription_name(text: &str) -> Result<Option<String>, Error> {
    if text.is_empty() || text.len() > 8192 {
        return Err(invalid("Use a subscription document of at most 8 KiB."));
    }
    let t = tokens(text.trim_start_matches('\u{feff}'))?;
    if t.first() != Some(&"subscription") {
        return Err(invalid(
            "Open one explicit subscription operation in a .graphql/.gql document.",
        ));
    }
    let mut at = 1;
    let operation = if t.get(at).is_some_and(|s| name(s)) {
        let value = Some(t[at].to_owned());
        at += 1;
        value
    } else {
        None
    };
    if !t.get(at).is_some_and(|s| matches!(*s, "(" | "@" | "{")) {
        return Err(invalid("Invalid subscription operation header."));
    }
    at = selection_end(&t, at)?;
    let mut fragments = Vec::new();
    while at < t.len() {
        if t.get(at) != Some(&"fragment")
            || !t.get(at + 1).is_some_and(|s| name(s))
            || t.get(at + 2) != Some(&"on")
            || !t.get(at + 3).is_some_and(|s| name(s))
        {
            return Err(invalid(
                "The document must contain exactly one subscription operation (fragments are allowed).",
            ));
        }
        if fragments.contains(&t[at + 1]) {
            return Err(invalid("Duplicate fragment declaration."));
        }
        fragments.push(t[at + 1]);
        at += 4;
        if !t.get(at).is_some_and(|s| matches!(*s, "@" | "{")) {
            return Err(invalid("Invalid fragment header."));
        }
        at = selection_end(&t, at)?;
    }
    Ok(operation)
}
fn endpoint(s: &str) -> Result<(), Error> {
    if s.len() > 2048
        || s.chars().any(|c| c.is_whitespace() || c.is_control())
        || s.contains(['\\', '#'])
    {
        return Err(invalid(
            "Use one literal absolute ws:// or wss:// endpoint without fragment or whitespace.",
        ));
    }
    let body = s
        .strip_prefix("ws://")
        .or_else(|| s.strip_prefix("wss://"))
        .ok_or_else(|| invalid("Use an explicit ws:// or wss:// endpoint."))?;
    let (authority, path) = body.split_once('/').ok_or_else(|| {
        invalid("Include the exact WebSocket path, including / for a root endpoint.")
    })?;
    if authority.is_empty() || authority.contains(['@', '?', '%']) || path.contains(['{', '}']) {
        return Err(invalid(
            "Endpoint credentials, variables and ambiguous authorities are refused; enter initialization payload separately.",
        ));
    }
    let (host, port) = if authority.starts_with('[') {
        let end = authority
            .find(']')
            .ok_or_else(|| invalid("Invalid bracketed IPv6 authority."))?;
        if !authority[1..end].contains(':')
            || !authority[1..end]
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || matches!(b, b':' | b'.'))
        {
            return Err(invalid("Invalid bracketed IPv6 authority."));
        }
        (&authority[..=end], &authority[end + 1..])
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, &authority[host.len()..host.len() + 1 + port.len()]),
            None => (authority, ""),
        }
    };
    if host.is_empty()
        || !host.starts_with('[')
            && !host
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-'))
    {
        return Err(invalid(
            "Use a canonical literal host or bracketed IPv6 address.",
        ));
    }
    if !port.is_empty()
        && (!port.starts_with(':')
            || port[1..].parse::<u16>().ok().filter(|p| *p > 0).is_none()
            || !port[1..].bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(invalid("Invalid explicit endpoint port."));
    }
    Ok(())
}
fn object(text: &str, label: &str) -> Result<Value, Error> {
    if text.len() > 4096 {
        return Err(invalid(
            "Variables and the connection payload must each be under 4 KiB.",
        ));
    }
    let value: Value = serde_json::from_str(text)
        .map_err(|_| invalid(&format!("{label} must be a JSON object.")))?;
    if !value.is_object() {
        return Err(invalid(&format!("{label} must be a JSON object.")));
    }
    Ok(value)
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Source {
    root: String,
    path: String,
    version: u64,
    text: String,
}
impl Source {
    fn capture(ctx: &Invocation) -> Result<Self, Error> {
        let project = ctx
            .project()
            .ok_or_else(|| invalid("Open the subscription document's project."))?;
        let root = project.root()?;
        let editor = ctx
            .editor()
            .ok_or_else(|| invalid("Open a subscription .graphql/.gql document first."))?;
        let path = editor.path();
        if root.is_empty()
            || root == "/"
            || !matches!(path.rsplit('.').next(), Some("graphql" | "gql"))
            || editor.language() != "graphql"
            || host::project_of(&path)
                .and_then(|p| p.root().ok())
                .as_deref()
                != Some(root.as_str())
        {
            return Err(invalid(
                "Use a current GraphQL document belonging to this project.",
            ));
        }
        let doc = editor.document()?;
        subscription_name(&doc.text)?;
        Ok(Self {
            root,
            path,
            version: doc.version,
            text: doc.text,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Config {
    files: Vec<(String, String)>,
    path: Option<String>,
    complete: bool,
}
impl Config {
    fn capture(project: &Project) -> Self {
        let mut out = Self {
            files: Vec::new(),
            path: None,
            complete: true,
        };
        let mut paths = Vec::new();
        for glob in [
            "**/application*.properties",
            "**/application*.yml",
            "**/application*.yaml",
        ] {
            match project.find_files(glob, 64) {
                Ok(found) if found.len() < 64 => paths.extend(found),
                _ => {
                    out.complete = false;
                    return out;
                }
            }
        }
        paths.sort();
        paths.dedup();
        if paths.len() > 64 {
            out.complete = false;
            return out;
        }
        let mut values = Vec::new();
        let mut bytes = 0;
        let root = project.root().unwrap_or_default();
        for path in paths {
            if host::project_of(&path)
                .and_then(|p| p.root().ok())
                .as_deref()
                != Some(root.as_str())
            {
                out.complete = false;
                return out;
            }
            let Ok(text) = project.read_file(&path) else {
                out.complete = false;
                return out;
            };
            bytes += text.len();
            if text.len() > 32768 || bytes > 128 * 1024 {
                out.complete = false;
                return out;
            }
            for key in spring_text::config::keys(&path, &text) {
                if [
                    "spring.graphql.websocket.path",
                    "server.servlet.context-path",
                ]
                .contains(&key.key.as_str())
                {
                    values.push((key.key, key.value))
                }
            }
            out.files.push((path, text));
        }
        let unique = |key: &str| {
            let mut values = values
                .iter()
                .filter(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
                .collect::<Vec<_>>();
            values.sort();
            values.dedup();
            if values.len() == 1 {
                Some(values.remove(0))
            } else {
                None
            }
        };
        if let Some(path) = unique("spring.graphql.websocket.path") {
            let context = unique("server.servlet.context-path").unwrap_or_default();
            let conflicts = values
                .iter()
                .filter(|(k, _)| k == "server.servlet.context-path")
                .map(|(_, v)| v)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                > 1;
            if !conflicts
                && [path.as_str(), context.as_str()].iter().all(|v| {
                    v.is_empty()
                        || v.starts_with('/')
                            && !v.starts_with("//")
                            && !v.contains(['$', '{', '}', '?', '#', '\\'])
                            && !v.chars().any(|c| c.is_whitespace() || c.is_control())
                })
                && !path.is_empty()
            {
                out.path = Some(format!(
                    "{}/{}",
                    context.trim_end_matches('/'),
                    path.trim_start_matches('/')
                ));
            }
        }
        out
    }
}
trait Running {
    fn poll(&self, max: u32) -> Result<host::Chunk, Error>;
    fn wait(&self, ms: u32) -> Result<host::Chunk, Error>;
    fn write(&self, text: &str) -> Result<(), Error>;
    fn kill(&self) -> Result<(), Error>;
}
impl Running for Child {
    fn poll(&self, max: u32) -> Result<host::Chunk, Error> {
        self.poll(max)
    }
    fn wait(&self, ms: u32) -> Result<host::Chunk, Error> {
        self.wait(ms)
    }
    fn write(&self, text: &str) -> Result<(), Error> {
        self.write(text)
    }
    fn kill(&self) -> Result<(), Error> {
        self.kill()
    }
}
trait Backend {
    fn probe(&mut self, project: &Project) -> Result<String, Error>;
    fn start(&mut self, project: &Project) -> Result<Box<dyn Running>, Error>;
}
const BOOTSTRAP: &str = "const a=process.argv.slice(1);const m=a.shift();process.argv=['node',...(m==='probe'?['--probe']:[])];eval(a.join(''))";
fn command(probe: bool) -> Command {
    let mut args = vec![
        "--eval".to_owned(),
        BOOTSTRAP.into(),
        "--".into(),
        if probe { "probe" } else { "run" }.into(),
    ];
    // The released native host caps every argv item at 4096 bytes. Chunk by
    // Unicode scalar count so each fragment is <=4000 bytes, preserving source.
    let chars = HELPER.chars().collect::<Vec<_>>();
    for chunk in chars.chunks(1000) {
        args.push(chunk.iter().collect::<String>());
    }
    Command::new("node").args(args)
}
struct Native;
impl Backend for Native {
    fn probe(&mut self, project: &Project) -> Result<String, Error> {
        let child = host::start(&command(true), Some(project))?;
        child.close_stdin()?;
        let task = host::begin_task("Check optional Node WebSocket runtime", true);
        let start = Instant::now();
        let mut output = String::new();
        loop {
            if task.is_cancelled() {
                let _ = child.kill();
                return Err(Error::Cancelled);
            }
            if start.elapsed() > Duration::from_secs(3) {
                let _ = child.kill();
                return Err(invalid("Node runtime check timed out."));
            }
            let chunk = child.wait(100)?;
            output.push_str(&chunk.stdout);
            if !chunk.stderr.is_empty() || output.len() > 1024 {
                let _ = child.kill();
                return Err(invalid("Node runtime check produced an unexpected result."));
            }
            if let Some(status) = chunk.status {
                if status != 0 {
                    return Err(invalid(
                        "Install/approve Node 22 or newer with its built-in WebSocket available.",
                    ));
                }
                return parse_probe(&output);
            }
        }
    }
    fn start(&mut self, project: &Project) -> Result<Box<dyn Running>, Error> {
        Ok(Box::new(host::start(&command(false), Some(project))?))
    }
}
fn parse_probe(output: &str) -> Result<String, Error> {
    let value: Value =
        serde_json::from_str(output).map_err(|_| invalid("Unexpected Node runtime proof."))?;
    let version = value["node"]
        .as_str()
        .ok_or_else(|| invalid("Missing Node version."))?;
    if value["protocol"] != "openidea-graphql-ws-v1"
        || value.as_object().is_none_or(|v| v.len() != 2)
        || version.len() > 64
        || !version
            .split('.')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        || version
            .split('.')
            .next()
            .and_then(|v| v.parse::<u32>().ok())
            .is_none_or(|v| v < 22)
    {
        return Err(invalid(
            "Node 22 or newer with built-in WebSocket is required.",
        ));
    }
    Ok(version.into())
}
struct Session {
    root: String,
    source: String,
    child: Box<dyn Running>,
    bytes: usize,
    pending: String,
    terminal: bool,
}
#[derive(Default)]
pub(crate) struct Runtime {
    session: Option<Session>,
}
impl Runtime {
    pub(crate) fn run_command(
        &mut self,
        id: &str,
        ctx: &Invocation,
    ) -> Result<Option<String>, Error> {
        self.run_with(id, ctx, &mut Native)
    }
    fn run_with(
        &mut self,
        id: &str,
        ctx: &Invocation,
        backend: &mut impl Backend,
    ) -> Result<Option<String>, Error> {
        let result = self.run_session(id, ctx, backend);
        let _ = host::set_context(ACTIVE_CONTEXT, self.session.is_some());
        result
    }
    fn run_session(
        &mut self,
        id: &str,
        ctx: &Invocation,
        backend: &mut impl Backend,
    ) -> Result<Option<String>, Error> {
        let Some(project) = ctx.project() else {
            if let Some(session) = self.session.take() {
                let _ = session.child.kill();
            }
            return Err(invalid(
                "Open the project that contains the subscription document. Any running subscription was stopped.",
            ));
        };
        let root = project.root()?;
        if self.session.as_ref().is_some_and(|s| s.root != root) {
            let s = self.session.take().ok_or(Error::Stale)?;
            let _ = s.child.kill();
            return Err(invalid(
                "The project changed, so the running subscription was stopped. Run the command again.",
            ));
        }
        match id {
            START => {
                if self.session.is_some() {
                    return Err(invalid(
                        "A subscription is already running. Run Spring Endpoints: Read GraphQL Subscription Output or Spring Endpoints: Cancel GraphQL Subscription first.",
                    ));
                }
                let source = Source::capture(ctx)?;
                let config = Config::capture(&project);
                let suggestion = config
                    .path
                    .as_ref()
                    .map(|p| format!("ws://localhost:8080{p}"));
                let Some(url) = host::input(
                    "GraphQL WebSocket URL",
                    suggestion.as_deref(),
                )?
                else {
                    return Ok(None);
                };
                endpoint(&url)?;
                let Some(variables) = host::input_multiline(
                    "Subscription variables (JSON object)",
                    Some("{}"),
                )?
                else {
                    return Ok(None);
                };
                let variables = object(&variables, "Variables")?;
                let Some(init) = host::input_multiline(
                    "Connection payload (JSON object, sent once)",
                    Some("{}"),
                )?
                else {
                    return Ok(None);
                };
                let init = object(&init, "Initialization payload")?;
                let operation = subscription_name(&source.text)?;
                let node = backend.probe(&project)?;
                let review = format!(
                    "Project: {}\nDocument: {}\nEndpoint: {}\nNode: {}\n\nSubscription:\n{}\n\nVariables:\n{}\n\nConnection payload (sent once):\n{}\n\nOnly the values above are sent: no credentials, headers or cookies from your application. Results appear in the {OUTPUT} output. The subscription stops after 120 seconds.",
                    source.root,
                    source.path,
                    url,
                    node,
                    ui_kit::prompts::readable_source(&source.text),
                    serde_json::to_string_pretty(&variables).unwrap_or_default(),
                    serde_json::to_string_pretty(&init).unwrap_or_default()
                );
                if !ui_kit::prompts::confirm_exact(
                    "Start this GraphQL subscription?",
                    &review,
                )? {
                    return Err(Error::Cancelled);
                }
                if backend.probe(&project)? != node
                    || Source::capture(ctx)? != source
                    || Config::capture(&project) != config
                {
                    return Err(Error::Stale);
                }
                let input=serde_json::to_string(&json!({"endpoint":url,"query":source.text,"operationName":operation,"variables":variables,"init":init})).map_err(|_|invalid("Cannot serialize subscription input."))?+"\n";
                if input.len() > 24 * 1024 {
                    return Err(invalid(
                        "The subscription, variables and connection payload are too large together (over 24 KiB).",
                    ));
                }
                let child = backend.start(&project)?;
                if let Err(error) = child.write(&input) {
                    let _ = child.kill();
                    return Err(error);
                }
                let _ = host::output_append(
                    OUTPUT,
                    &format!(
                        "\nSubscription document: {}\nNode {node}. Stops after 120 seconds. Variables and payloads are not logged.\n",
                        source.path
                    ),
                );
                let _ = host::output_reveal(OUTPUT);
                self.session = Some(Session {
                    root: source.root,
                    source: source.path,
                    child,
                    bytes: 0,
                    pending: String::new(),
                    terminal: false,
                });
                Ok(Some("Subscription started. Run Spring Endpoints: Read GraphQL Subscription Output to see results, or Spring Endpoints: Cancel GraphQL Subscription to stop it. It stops on its own after 120 seconds.".into()))
            }
            POLL => {
                let Some(session) = self.session.as_mut() else {
                    return Err(invalid(NOT_RUNNING));
                };
                let chunk = session.child.poll(256 * 1024)?;
                if let Err(error) = append(session, &chunk) {
                    self.session = None;
                    return Err(error);
                }
                // Since 0.1.50 the exit status arrives only with the last output chunk.
                let result = if let Some(status) = chunk.status {
                    let terminal = session.terminal;
                    self.session = None;
                    format!(
                        "The subscription ended (exit status {status}). {} Check the output for errors.",
                        if terminal {
                            "All output has been read."
                        } else {
                            "It stopped before reporting how it ended."
                        }
                    )
                } else {
                    "The subscription is running. Read the output again for new results.".into()
                };
                let _ = host::output_reveal(OUTPUT);
                Ok(Some(result))
            }
            CANCEL => {
                let Some(mut session) = self.session.take() else {
                    return Err(invalid(NOT_RUNNING));
                };
                let sent = session.child.write("{\"type\":\"cancel\"}\n").is_ok();
                let chunk = if sent {
                    session.child.wait(1500).ok()
                } else {
                    None
                };
                if let Some(chunk) = &chunk {
                    append(&mut session, chunk)?;
                }
                let forced = chunk.as_ref().is_none_or(|c| c.status.is_none());
                if forced {
                    session.child.kill()?;
                }
                Ok(Some(if forced{"Stopped the subscription. The server did not confirm that it closed the subscription."}else{"Subscription cancelled."}.into()))
            }
            _ => Err(invalid("Unknown GraphQL subscription command.")),
        }
    }
}
fn append(session: &mut Session, chunk: &host::Chunk) -> Result<(), Error> {
    // `terminal` tells a clean end (the helper's final record arrived) from a crash.
    session.pending.push_str(&chunk.stdout);
    if session.pending.len() > 256 * 1024 {
        session.child.kill()?;
        return Err(invalid("Subscription protocol line exceeded its bound."));
    }
    while let Some(end) = session.pending.find('\n') {
        let line = session.pending.drain(..=end).collect::<String>();
        if serde_json::from_str::<Value>(&line)
            .ok()
            .and_then(|v| v["type"].as_str().map(str::to_owned))
            .is_some_and(|kind| {
                matches!(
                    kind.as_str(),
                    "failure"
                        | "lifetime-ended"
                        | "client-cancelled"
                        | "client-input-closed"
                        | "output-limit"
                        | "operation-error"
                        | "server-complete"
                        | "connection-closed"
                )
            })
        {
            session.terminal = true;
        }
    }
    let data = format!("{}{}", chunk.stdout, chunk.stderr);
    let mut shown = data
        .len()
        .min((256 * 1024usize).saturating_sub(session.bytes));
    while !data.is_char_boundary(shown) {
        shown -= 1
    }
    if shown > 0 {
        host::output_append(
            OUTPUT,
            &format!("\n{}:\n{}", session.source, &data[..shown]),
        )?;
        session.bytes += shown;
    }
    if shown < data.len() {
        session.child.kill()?;
        return Err(invalid(
            "Subscription process exceeded the 256 KiB display bound and was ended.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};
    #[test]
    fn admits_one_subscription_and_fragments_but_no_extra_operations_or_incomplete_document() {
        assert_eq!(subscription_name("# query hidden\nsubscription Events($s: String = \"mutation {x}\") {event(value:$s){...Value}} fragment Value on Event { value }").unwrap(),Some("Events".into()));
        assert_eq!(subscription_name("subscription { event }").unwrap(), None);
        for text in [
            "query Q {event}",
            "mutation {event}",
            "subscription S {event} query Q {other}",
            "subscription S {event} subscription T{other}",
            "subscription S {event",
            "subscription S {event(value:\"bad)}",
            "subscription S {}",
            "subscription S($a:[String!) {event}",
            "subscription S {event} fragment F on Event{value} fragment F on Event{value}",
        ] {
            assert!(subscription_name(text).is_err(), "{text}");
        }
        assert!(
            subscription_name(&format!(
                "subscription {{{}value{}}}",
                "{".repeat(65),
                "}".repeat(65)
            ))
            .is_err()
        );
    }
    #[test]
    fn exact_endpoint_and_explicit_payload_validation_refuse_ambiguous_authorities() {
        for url in [
            "ws://localhost:8080/graphql",
            "wss://example.test/subscriptions?key=explicit",
            "ws://[::1]:8080/graphql",
        ] {
            endpoint(url).unwrap();
        }
        for url in [
            "http://localhost/graphql",
            "//localhost/graphql",
            "ws://user:password@host/graphql",
            "ws://host",
            "ws://host:0/graphql",
            "ws://host:99999/graphql",
            "ws://host/path#fragment",
            "ws://host/{$path}",
            "ws://host/white space",
            "ws://host\\evil/graphql",
            "ws://host%2f.evil/path",
        ] {
            assert!(endpoint(url).is_err(), "{url}");
        }
        assert_eq!(object("{}", "Variables").unwrap(), json!({}));
        for value in ["null", "[]", "{bad}"] {
            assert!(object(value, "Variables").is_err());
        }
        for proof in [
            "{}",
            r#"{"protocol":"openidea-graphql-ws-v1","node":"21.7.0"}"#,
            r#"{"protocol":"foreign","node":"24.18.0"}"#,
        ] {
            assert!(parse_probe(proof).is_err());
        }
        assert_eq!(
            parse_probe(r#"{"protocol":"openidea-graphql-ws-v1","node":"24.18.0"}"#).unwrap(),
            "24.18.0"
        );
    }
    #[test]
    fn native_argv_fragments_obey_released_host_limits_and_reconstruct_exact_embedded_source() {
        for probe in [true, false] {
            let command = command(probe);
            assert_eq!(command.program, "node");
            assert!(command.args.len() < 128);
            assert!(
                command
                    .args
                    .iter()
                    .all(|arg| arg.len() <= 4096 && !arg.contains('\0'))
            );
            assert_eq!(command.args[3], if probe { "probe" } else { "run" });
            assert_eq!(command.args[4..].join(""), HELPER);
            assert!(command.env.is_empty());
        }
    }
    #[test]
    fn denied_optional_node_execution_leaves_no_connection_or_session() {
        let (h, mut runtime, _) = setup();
        answers(&h, true);
        assert!(matches!(
            runtime.run_command(START, &h.invocation()),
            Err(Error::Denied(_))
        ));
        assert!(runtime.session.is_none());
        assert!(h.requests().is_empty());
    }
    #[derive(Default)]
    struct State {
        writes: Vec<String>,
        kills: usize,
        starts: usize,
        waits: Vec<u32>,
        chunks: VecDeque<host::Chunk>,
        wait_status: Option<i32>,
    }
    struct Controlled(Rc<RefCell<State>>);
    impl Running for Controlled {
        fn poll(&self, _: u32) -> Result<host::Chunk, Error> {
            Ok(self.0.borrow_mut().chunks.pop_front().unwrap_or_default())
        }
        fn wait(&self, ms: u32) -> Result<host::Chunk, Error> {
            let mut state = self.0.borrow_mut();
            state.waits.push(ms);
            Ok(host::Chunk {
                status: state.wait_status,
                ..Default::default()
            })
        }
        fn write(&self, text: &str) -> Result<(), Error> {
            self.0.borrow_mut().writes.push(text.into());
            Ok(())
        }
        fn kill(&self) -> Result<(), Error> {
            self.0.borrow_mut().kills += 1;
            Ok(())
        }
    }
    struct Fake {
        state: Rc<RefCell<State>>,
        probes: usize,
        after_probe: Option<Box<dyn FnMut(usize)>>,
    }
    impl Backend for Fake {
        fn probe(&mut self, _: &Project) -> Result<String, Error> {
            self.probes += 1;
            if let Some(callback) = self.after_probe.as_mut() {
                callback(self.probes)
            }
            Ok("24.18.0".into())
        }
        fn start(&mut self, _: &Project) -> Result<Box<dyn Running>, Error> {
            self.state.borrow_mut().starts += 1;
            Ok(Box::new(Controlled(self.state.clone())))
        }
    }
    #[test]
    fn read_and_cancel_commands_are_enabled_only_while_a_subscription_runs() {
        let manifest = include_str!("../plugin.toml");
        for id in [POLL, CANCEL] {
            let command = manifest.split("[[contributes.commands]]").find(|c| c.contains(&format!("id = \"{id}\""))).unwrap();
            assert!(command.contains(&format!("when = \"plugin.aritti.spring-web.{ACTIVE_CONTEXT}\"")), "{id}");
        }
        let (h, mut runtime, mut backend) = setup();
        let error = runtime.run_with(POLL, &h.invocation(), &mut backend).unwrap_err();
        assert!(matches!(error, Error::Invalid(ref m) if m.contains("Start GraphQL Subscription")));
    }
    fn setup() -> (Rc<MockHost>, Runtime, Fake) {
        let h = Rc::new(MockHost::install().with_capabilities(&["project.read", "editor.read"]));
        h.add_file("pom.xml", "spring-boot-starter-graphql");
        h.open_editor(
            "/project/events.graphql",
            "graphql",
            "subscription Events($id: ID!) {event(id:$id){value}}",
            &[],
        );
        let state = Rc::new(RefCell::new(State::default()));
        (
            h,
            Runtime::default(),
            Fake {
                state,
                probes: 0,
                after_probe: None,
            },
        )
    }
    fn answers(h: &MockHost, confirm: bool) {
        h.answer_next_prompt(Some(&["ws://localhost:8080/graphql"]));
        h.answer_next_prompt(Some(&["{\"id\":\"explicit\"}"]));
        h.answer_next_prompt(Some(&["{\"token\":\"explicit-fixture-value\"}"]));
        h.answer_next_prompt(if confirm { Some(&["yes"]) } else { None });
    }
    #[test]
    fn explicit_reviewed_start_uses_current_source_and_stdin_without_credential_reuse_or_logging() {
        let (h, mut runtime, mut backend) = setup();
        answers(&h, true);
        runtime
            .run_with(START, &h.invocation(), &mut backend)
            .unwrap();
        assert_eq!(backend.probes, 2);
        let state = backend.state.borrow();
        assert_eq!(state.starts, 1);
        let payload: Value = serde_json::from_str(&state.writes[0]).unwrap();
        assert_eq!(payload["endpoint"], "ws://localhost:8080/graphql");
        assert_eq!(payload["operationName"], "Events");
        assert_eq!(payload["variables"], json!({"id":"explicit"}));
        assert_eq!(payload["init"], json!({"token":"explicit-fixture-value"}));
        assert!(
            payload["query"]
                .as_str()
                .unwrap()
                .contains("subscription Events")
        );
        assert!(!h.output()[OUTPUT].contains("explicit-fixture-value"));
        assert!(h.process_runs().is_empty());
        assert!(h.requests().is_empty());
    }
    #[test]
    fn cancellation_and_stale_source_rechecks_never_start_an_unreviewed_connection() {
        let (h, mut runtime, mut backend) = setup();
        answers(&h, false);
        assert!(matches!(
            runtime.run_with(START, &h.invocation(), &mut backend),
            Err(Error::Cancelled)
        ));
        assert_eq!(backend.state.borrow().starts, 0);
        let (h, mut runtime, mut backend) = setup();
        let changed = h.clone();
        backend.after_probe = Some(Box::new(move |count| {
            if count == 2 {
                changed.type_in_editor("# changed during runtime check");
            }
        }));
        answers(&h, true);
        assert!(matches!(
            runtime.run_with(START, &h.invocation(), &mut backend),
            Err(Error::Stale)
        ));
        assert_eq!(backend.state.borrow().starts, 0);
    }
    #[test]
    fn graceful_cancel_writes_complete_control_before_bounded_wait_and_kill_fallback() {
        for (status, kills) in [(Some(0), 0), (None, 1)] {
            let (h, mut runtime, mut backend) = setup();
            answers(&h, true);
            runtime
                .run_with(START, &h.invocation(), &mut backend)
                .unwrap();
            assert_eq!(h.context_keys()[ACTIVE_CONTEXT], openidea_plugin::ContextValue::Flag(true));
            backend.state.borrow_mut().wait_status = status;
            runtime
                .run_with(CANCEL, &h.invocation(), &mut backend)
                .unwrap();
            assert_eq!(h.context_keys()[ACTIVE_CONTEXT], openidea_plugin::ContextValue::Flag(false));
            let state = backend.state.borrow();
            assert_eq!(state.writes[1], "{\"type\":\"cancel\"}\n");
            assert_eq!(state.waits, vec![1500]);
            assert_eq!(state.kills, kills);
            assert!(runtime.session.is_none());
        }
    }
    #[test]
    fn owned_poll_displays_protocol_results_and_retires_exit_without_claiming_operation_success() {
        let (h, mut runtime, mut backend) = setup();
        answers(&h, true);
        runtime
            .run_with(START, &h.invocation(), &mut backend)
            .unwrap();
        backend.state.borrow_mut().chunks.push_back(host::Chunk {
            stdout: "{\"type\":\"next\",\"payload\":{\"data\":{\"value\":1}}}\n{\"type\":\"server-complete\"}\n".into(),
            status: Some(0),
            ..Default::default()
        });
        let result = runtime
            .run_with(POLL, &h.invocation(), &mut backend)
            .unwrap()
            .unwrap();
        assert!(result.contains("All output has been read."));
        assert!(h.output()[OUTPUT].contains("value"));
        assert!(runtime.session.is_none());
    }
    #[test]
    fn exit_status_means_all_output_was_read_even_without_final_record() {
        let (h, mut runtime, mut backend) = setup();
        answers(&h, true);
        runtime
            .run_with(START, &h.invocation(), &mut backend)
            .unwrap();
        backend.state.borrow_mut().chunks.push_back(host::Chunk {
            stdout: "{\"type\":\"next\",\"payload\":{\"data\":{}}}\n".into(),
            status: Some(0),
            ..Default::default()
        });
        let result = runtime
            .run_with(POLL, &h.invocation(), &mut backend)
            .unwrap()
            .unwrap();
        assert!(result.contains("stopped before reporting how it ended"));
        assert!(runtime.session.is_none());
    }
    #[test]
    fn configured_path_suggestion_requires_complete_unique_literal_local_configuration() {
        let (h, _, _) = setup();
        h.add_file(
            "src/main/resources/application.properties",
            "spring.graphql.websocket.path=/events\nserver.servlet.context-path=/app\n",
        );
        let project = h.invocation().project().unwrap();
        let config = Config::capture(&project);
        assert!(config.complete);
        assert_eq!(config.path.as_deref(), Some("/app/events"));
        h.add_file(
            "src/main/resources/application-dev.yml",
            "spring:\n  graphql:\n    websocket:\n      path: /other\n",
        );
        assert_eq!(Config::capture(&project).path, None);
    }
    #[test]
    fn oversized_exact_review_is_refused_before_any_connection_start() {
        let (h, mut runtime, mut backend) = setup();
        h.open_editor(
            "/project/events.graphql",
            "graphql",
            &format!("subscription {{event}} #{}", "x".repeat(8100)),
            &[],
        );
        h.answer_next_prompt(Some(&["ws://localhost:8080/graphql"]));
        let payload = serde_json::to_string(&json!({"value":"x".repeat(4000)})).unwrap();
        h.answer_next_prompt(Some(&[&payload]));
        h.answer_next_prompt(Some(&[&payload]));
        h.answer_next_prompt(Some(&["yes"]));
        assert!(matches!(
            runtime.run_with(START, &h.invocation(), &mut backend),
            Err(Error::Invalid(_))
        ));
        assert_eq!(backend.state.borrow().starts, 0);
        assert!(runtime.session.is_none());
    }
    #[test]
    fn non_graphql_source_wrong_project_and_oversized_review_do_not_start() {
        let (h, mut runtime, mut backend) = setup();
        h.open_editor("/project/C.java", "java", "subscription {event}", &[]);
        assert!(
            runtime
                .run_with(START, &h.invocation(), &mut backend)
                .is_err()
        );
        assert_eq!(backend.state.borrow().starts, 0);
        let (h, mut runtime, mut backend) = setup();
        h.open_editor(
            "/outside/events.graphql",
            "graphql",
            "subscription {event}",
            &[],
        );
        assert!(
            runtime
                .run_with(START, &h.invocation(), &mut backend)
                .is_err()
        );
        assert_eq!(backend.state.borrow().starts, 0);
        let (h, mut runtime, mut backend) = setup();
        h.open_editor(
            "/project/events.graphql",
            "graphql",
            &format!("subscription {{event(value:\"{}\")}}", "x".repeat(8192)),
            &[],
        );
        assert!(
            runtime
                .run_with(START, &h.invocation(), &mut backend)
                .is_err()
        );
        assert_eq!(backend.state.borrow().starts, 0);
    }
}
