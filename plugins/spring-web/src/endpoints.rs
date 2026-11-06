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
        let (Some(s), Some(dot), Some(nest), Some(open)) =
            (t.string(), toks.get(i + 1), toks.get(i + 2), toks.get(i + 3))
        else {
            continue;
        };
        if dot.s == "." && nest.s == "nest" && open.s == "{" {
            let to = toks[java::closing(&toks, i + 3)].end();
            out.push(Scope { from: open.at, header_end: open.at, to, prefix: Some(s.value) });
        }
    }
    out
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
    fn annotated_mappings_are_endpoints_too() {
        let src = "@RestController @RequestMapping(\"/u\") class U { @GetMapping(\"/{id}\") void get() {} @RequestMapping void any() {} }";
        assert_eq!(found("U.java", src), pairs(&[("GET", "/u/{id}"), ("", "/u")]));
    }
}
