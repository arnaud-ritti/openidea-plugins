//! The HTTP endpoints of a source file: annotated mappings and WebFlux /
//! WebMvc.fn functional routes (Java and the Kotlin `router {}` DSL).

use openidea_plugin::Range;
use spring_text::java::{self, Call};
use spring_text::{is_kotlin, parse_source, range, source_calls};

pub const VERBS: [&str; 7] = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];

/// A method reference naming a route's handler: `this::list`, `Handler::list`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handler {
    /// The class written before `::`; none for `this::` and Kotlin `::`.
    pub ty: Option<String>,
    pub method: String,
    pub at: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    /// Empty: any HTTP method.
    pub verb: String,
    pub path: String,
    pub range: Range,
    pub handler: Option<Handler>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MethodDecl {
    pub ty: String,
    pub name: String,
    pub range: Range,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileInfo {
    pub endpoints: Vec<Endpoint>,
    pub methods: Vec<MethodDecl>,
}

pub fn file_info(path: &str, text: &str) -> Option<FileInfo> {
    let file = parse_source(path, text);
    let mut info = FileInfo::default();
    for class in &file.types {
        for m in class.methods() {
            let name_range = range(text, m.at, m.at + m.name.len());
            info.methods.push(MethodDecl { ty: class.name.clone(), name: m.name.clone(), range: name_range });
            if !class.has(&["Controller", "RestController"])
                || class.annotation("RequestMapping").is_some_and(|a| !crate::literal_paths(a))
            {
                continue;
            }
            let maps: Vec<_> = m.annotations.iter().filter(|a| crate::MAPPINGS.contains(&a.name.as_str())).collect();
            if maps.is_empty() || !maps.iter().all(|a| crate::literal_paths(a)) {
                continue;
            }
            for (verb, path) in java::mappings(class, m) {
                info.endpoints.push(Endpoint { verb, path, range: name_range, handler: None });
            }
        }
    }
    info.endpoints.extend(routes(path, text));
    (!info.endpoints.is_empty() || !info.methods.is_empty()).then_some(info)
}

/// A `path("/api", …)` / `nest(path("/api"), …)` / Kotlin `"/api".nest { … }` scope.
struct Scope {
    from: usize,
    /// Where the scope's routes start (after the predicate).
    header_end: usize,
    to: usize,
    prefix: Option<String>,
}

fn scopes(calls: &[Call], text: &str, kotlin: bool) -> Vec<Scope> {
    let toks = java::tokens(text);
    let mut out = Vec::new();
    for c in calls.iter().filter(|c| matches!(c.name.as_str(), "path" | "nest" | "andNest") && !c.args.is_empty()) {
        let first = &c.args[0];
        let prefix = match &first.string {
            Some(s) => Some(s.value.clone()),
            None if first.text.starts_with("path(") || first.text.starts_with("RequestPredicates.path(") => {
                java::tokens(&first.text).iter().find_map(|t| t.string()).map(|s| s.value)
            }
            None => None,
        };
        let mut to = c.end;
        if kotlin {
            let next = toks.partition_point(|t| t.at < c.end);
            if toks.get(next).is_some_and(|t| t.s == "{") {
                to = toks[java::closing(&toks, next)].end();
            }
        }
        out.push(Scope { from: c.at, header_end: first.end, to, prefix });
    }
    for (i, t) in toks.iter().enumerate() {
        let Some(open) = toks.get(i + 1).filter(|o| o.s == "{") else { continue };
        if t.s != "nest" {
            continue;
        }
        let to = toks[java::closing(&toks, i + 1)].end();
        out.push(Scope { from: open.at, header_end: open.at, to, prefix: kotlin_nest_prefix(&toks, i) });
    }
    out
}

/// The prefix of a Kotlin `… nest { }` at token `i`: `"/a".nest`, `"/a" nest` or `path("/a").nest`; none for any other predicate.
fn kotlin_nest_prefix(toks: &[java::Tok], i: usize) -> Option<String> {
    let before = i.checked_sub(1).map(|k| toks[k])?;
    let before = if before.s == "." { toks.get(i.checked_sub(2)?).copied()? } else { before };
    if let Some(s) = before.string() {
        return Some(s.value);
    }
    let close = i.checked_sub(if toks[i - 1].s == "." { 2 } else { 1 })?;
    if toks[close].s != ")" {
        return None;
    }
    let open = (0..close).rev().find(|&k| toks[k].s == "(" && java::closing(toks, k) == close)?;
    let [name, _, arg, _] = [toks.get(open.checked_sub(1)?)?, &toks[open], toks.get(open + 1)?, toks.get(open + 2)?];
    (name.s == "path" && open + 2 == close).then(|| arg.string().map(|s| s.value)).flatten()
}

/// The path prefix at `at`; none when a predicate is not a plain path or `at` is part of one.
fn prefix_at(scopes: &[Scope], at: usize) -> Option<String> {
    let mut inside: Vec<&Scope> = scopes.iter().filter(|s| s.from < at && at < s.to).collect();
    if inside.iter().any(|s| at < s.header_end) {
        return None;
    }
    inside.sort_by_key(|s| s.from);
    let mut prefix = String::new();
    for s in inside {
        prefix = java::join_paths(&prefix, s.prefix.as_deref()?);
    }
    Some(prefix)
}

pub fn method_ref(arg: &java::CallArg) -> Option<Handler> {
    let (left, right) = arg.text.split_once("::")?;
    let ident = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_');
    if !ident(right) {
        return None;
    }
    let ty = match left.rsplit('.').next().unwrap_or("") {
        "" | "this" => None,
        t if ident(t) && t.starts_with(char::is_uppercase) => Some(t.to_owned()),
        _ => return None,
    };
    Some(Handler { ty, method: right.to_owned(), at: arg.at, end: arg.end })
}

fn routes(path: &str, text: &str) -> Vec<Endpoint> {
    if !["RouterFunction", "RequestPredicates", "coRouter", "router"].iter().any(|k| text.contains(k)) {
        return Vec::new();
    }
    let calls = source_calls(path, text);
    let scopes = scopes(&calls, text, is_kotlin(path));
    let mut out = Vec::new();
    for v in calls.iter().filter(|c| VERBS.contains(&c.name.as_str())) {
        let Some(literal) = v.args.first().and_then(|a| a.string.as_ref()) else {
            continue;
        };
        let Some(prefix) = prefix_at(&scopes, v.at) else {
            continue;
        };
        let handler_arg = if v.args.len() >= 2 {
            v.args.last()
        } else {
            let route = calls.iter().find(|r| {
                matches!(r.name.as_str(), "route" | "andRoute")
                    && r.args.len() >= 2
                    && r.args[0].at <= v.at
                    && v.at < r.args[0].end
            });
            route.and_then(|r| r.args.last())
        };
        out.push(Endpoint {
            verb: v.name.clone(),
            path: java::join_paths(&prefix, &literal.value),
            range: range(text, v.at, v.at + v.name.len()),
            handler: handler_arg.and_then(method_ref),
        });
    }
    out
}

/// A request a test sends to a literal URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub verb: String,
    /// The URL's path, without query or fragment.
    pub path: String,
    pub at: usize,
    pub end: usize,
}

const TEMPLATE_METHODS: [(&str, &str); 9] = [
    ("getForObject", "GET"),
    ("getForEntity", "GET"),
    ("postForObject", "POST"),
    ("postForEntity", "POST"),
    ("postForLocation", "POST"),
    ("put", "PUT"),
    ("delete", "DELETE"),
    ("patchForObject", "PATCH"),
    ("headForHeaders", "HEAD"),
];

/// The URL literals of MockMvc, WebTestClient/WebClient/RestClient and
/// RestTemplate calls in a test source.
pub fn requests(path: &str, text: &str) -> Vec<Request> {
    if !path.contains("/src/test/") {
        return Vec::new();
    }
    let mock = text.contains("MockMvc");
    let client = ["WebTestClient", "RestTestClient", "WebClient", "RestClient"].iter().any(|k| text.contains(k));
    let template = text.contains("RestTemplate");
    if !(mock || client || template) {
        return Vec::new();
    }
    let calls = source_calls(path, text);
    let mut out = Vec::new();
    for c in calls.iter().filter(|c| !c.is_new || is_kotlin(path)) {
        let Some(url) = c.args.first().and_then(|a| a.string.as_ref()) else { continue };
        let templated = c.receiver.as_ref().is_some_and(|r| {
            let r = r.to_lowercase();
            r.contains("template") || r.contains("rest")
        });
        let lower = c.name.to_uppercase();
        let verb = if mock
            && !c.chained
            && VERBS.contains(&lower.as_str())
            && c.name == c.name.to_lowercase()
            && c.receiver.as_ref().is_none_or(|r| r == "MockMvcRequestBuilders" || r.to_lowercase().contains("mvc"))
        {
            lower
        } else if client && c.name == "uri" {
            let before = calls.iter().find(|p| {
                p.args.is_empty()
                    && VERBS.contains(&p.name.to_uppercase().as_str())
                    && p.name == p.name.to_lowercase()
                    && text.get(p.end..c.at).is_some_and(|gap| gap.trim() == ".")
            });
            let Some(before) = before else { continue };
            before.name.to_uppercase()
        } else if template && !templated {
            continue;
        } else if template && c.name == "exchange" {
            let Some(method) = c.args.get(1) else { continue };
            let verb = method.text.rsplit('.').next().unwrap_or("").to_owned();
            if !VERBS.contains(&verb.as_str()) {
                continue;
            }
            verb
        } else if template && let Some((_, verb)) = TEMPLATE_METHODS.iter().find(|(n, _)| *n == c.name) {
            verb.to_string()
        } else {
            continue;
        };
        let path = url.value.split(['?', '#']).next().unwrap_or("");
        if path.starts_with('/') {
            out.push(Request { verb, path: path.to_owned(), at: url.at, end: url.end });
        }
    }
    out
}

/// Whether a request path reaches a mapping's URI template; a `{variable}`
/// matches any segment.
pub fn matches(mapping: &str, request: &str) -> bool {
    let segments = |p: &str| p.split('/').filter(|s| !s.is_empty()).map(str::to_owned).collect::<Vec<_>>();
    let (wanted, got) = (segments(mapping), segments(request));
    for (i, seg) in wanted.iter().enumerate() {
        if seg == "**" || seg.starts_with("{*") {
            return true;
        }
        let Some(part) = got.get(i) else { return false };
        if seg == "*" {
            continue;
        }
        let Some(open) = seg.find('{') else {
            if seg != part {
                return false;
            }
            continue;
        };
        let (prefix, suffix) = (&seg[..open], seg.rsplit_once('}').map_or("", |x| x.1));
        if !(part.len() >= prefix.len() + suffix.len() && part.starts_with(prefix) && part.ends_with(suffix)) {
            return false;
        }
    }
    wanted.len() == got.len()
}

/// Whether a mapping serves a request (`HEAD` falls back to `GET`).
pub fn serves(endpoint: &Endpoint, request: &Request) -> bool {
    (endpoint.verb.is_empty() || endpoint.verb == request.verb || (request.verb == "HEAD" && endpoint.verb == "GET"))
        && matches(&endpoint.path, &request.path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const JAVA_ROUTES: &str = r#"package com.acme;

@Configuration
class Routes {
    @Bean
    RouterFunction<ServerResponse> routes(UserHandler h) {
        return RouterFunctions.route().GET("/x", this::list).POST("/y", h::create)
            .path("/api", b -> b.GET("/users", Handler::list).DELETE("/users/{id}", handler))
            .nest(path("/v2"), b -> b.PUT("/z", this::list))
            .nest(GET("/pred"), b -> b.GET("/inner", this::list))
            .build();
    }

    RouterFunction<ServerResponse> old() {
        return RouterFunctions.route(RequestPredicates.GET("/old"), this::list).andRoute(POST("/o2"), this::list);
    }

    ServerResponse list(ServerRequest r) { return null; }
}
"#;

    const KOTLIN_ROUTES: &str = r#"package com.acme

@Configuration
class Routes(private val h: UserHandler) {
    @Bean
    fun routes() = coRouter {
        GET("/x", ::list)
        "/api".nest {
            GET("/users")
            POST("/users/{id}", h::create)
        }
        nest(path("/v2")) { PUT("/z") }
    }

    suspend fun list(r: ServerRequest) = ok().buildAndAwait()
}
"#;

    fn found(path: &str, text: &str) -> Vec<(String, String)> {
        let info = file_info(path, text).unwrap();
        info.endpoints.into_iter().map(|e| (e.verb, e.path)).collect()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(v, p)| (v.to_string(), p.to_string())).collect()
    }

    #[test]
    fn router_function_get_registers_endpoint() {
        assert_eq!(
            found("A.java", JAVA_ROUTES),
            pairs(&[
                ("GET", "/x"),
                ("POST", "/y"),
                ("GET", "/api/users"),
                ("DELETE", "/api/users/{id}"),
                ("PUT", "/v2/z"),
                ("GET", "/old"),
                ("POST", "/o2"),
            ])
        );
        assert_eq!(
            found("A.kt", KOTLIN_ROUTES),
            pairs(&[("GET", "/x"), ("GET", "/api/users"), ("POST", "/api/users/{id}"), ("PUT", "/v2/z")])
        );
    }

    #[test]
    fn router_function_handlers_are_method_references() {
        let handlers = |path: &str, text: &str| -> Vec<Option<(Option<String>, String)>> {
            let info = file_info(path, text).unwrap();
            info.endpoints.into_iter().map(|e| e.handler.map(|h| (h.ty, h.method))).collect()
        };
        let list = |ty: Option<&str>| Some((ty.map(str::to_owned), "list".to_owned()));
        assert_eq!(
            handlers("A.java", JAVA_ROUTES),
            [list(None), None, list(Some("Handler")), None, list(None), list(None), list(None)]
        );
        assert_eq!(handlers("A.kt", KOTLIN_ROUTES), [list(None), None, None, None]);
    }

    #[test]
    fn kotlin_dot_nest_routes_are_not_registered_without_prefix() {
        let src = r#"fun routes() = router {
    "/ok".nest { GET("/a") }
    path("/p").nest { GET("/b") }
    "/i" nest { GET("/c") }
    ("/x" and accept(APPLICATION_JSON)).nest { GET("/d") }
    accept(APPLICATION_JSON).nest { GET("/e") }
}"#;
        assert_eq!(found("R.kt", src), pairs(&[("GET", "/ok/a"), ("GET", "/p/b"), ("GET", "/i/c")]));
    }

    #[test]
    fn matches_uri_templates() {
        assert!(matches("/users/{id}", "/users/42"));
        assert!(matches("/users/{id}", "/users/{id}/"));
        assert!(matches("/users/{id:[0-9]+}/x", "/users/1/x"));
        assert!(matches("/f/{*rest}", "/f/a/b"));
        assert!(matches("/a/*/c", "/a/b/c"));
        assert!(matches("/by-{n}.json", "/by-x.json"));
        assert!(!matches("/users/{id}", "/users"));
        assert!(!matches("/users", "/users/1"));
        assert!(!matches("/users", "/other"));
    }

    #[test]
    fn reads_test_requests() {
        let src = r#"class T {
            void t() {
                mockMvc.perform(get("/a/{id}", 1)).andExpect(x);
                mockMvc.perform(MockMvcRequestBuilders.patch("/b"));
                webTestClient.post().uri("/c?q=1").exchange();
                map.get("/not");
                client.get().uri("http://host/d");
                rest.headForHeaders("/e");
            }
        } // MockMvc WebTestClient RestTemplate"#;
        let found: Vec<_> = requests("/p/src/test/java/T.java", src).into_iter().map(|r| (r.verb, r.path)).collect();
        assert_eq!(
            found,
            [
                ("GET".into(), "/a/{id}".into()),
                ("PATCH".into(), "/b".into()),
                ("POST".into(), "/c".into()),
                ("HEAD".into(), "/e".into())
            ]
        );
        assert!(requests("/p/src/main/java/T.java", src).is_empty());
    }

    #[test]
    fn annotated_mappings_are_endpoints_too() {
        let src = "@RestController @RequestMapping(\"/u\") class U { @GetMapping(\"/{id}\") void get() {} @RequestMapping void any() {} }";
        assert_eq!(found("U.java", src), pairs(&[("GET", "/u/{id}"), ("", "/u")]));
    }
}
