//! Spring Web Tools for OpenIDEA: checks `@PathVariable` parameters against
//! the URI templates of their request mappings, and links the view names a
//! `@Controller` returns to their template files, and links functional routes
//! and test URLs to their handlers. Text-based.

pub(crate) const ROUTE_MARKER: &str = "icons/marker-route.svg";
pub(crate) const TEMPLATE_MARKER: &str = "icons/marker-template.svg";

use endpoints::{FileInfo, Handler};
use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::{offset, position};
use openidea_plugin::views::{Body, Button, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Diagnostic, Error, Invocation, Location, NotificationLevel, Plugin, Position, Project, Range, Severity, TextEdit, host,
};
use spring_text::index::{COMMAND_BUDGET, Index, Indexes, VIEW_BUDGET, project_of};
use spring_text::java::{self, Annotation, JavaFile, Param, Str};
use spring_text::{SOURCE_GLOBS, config, contains, is_kotlin, parse_source, range, source_calls};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};
use ui_kit::routes::{self, Handler as RouteHandler, Route, RouteEvent, RouteList};

mod checks;
mod endpoints;
mod generate;
mod openapi_export;
mod dto_schemas;
mod graphql_subscriptions;
mod subscription_drafts;
mod feign_requests;
mod template_links;
mod source_lenses;
mod model_links;
mod request_examples;
mod window;
mod xml_checks;

pub(crate) const MAPPINGS: [&str; 6] = ["RequestMapping", "GetMapping", "PostMapping", "PutMapping", "DeleteMapping", "PatchMapping"];
/// Template engines Spring Boot auto-configures, looked up under `templates/`.
const TEMPLATE_EXTENSIONS: [&str; 7] = ["html", "ftlh", "ftl", "mustache", "peb", "jte", "kte"];
/// Parameter types Spring fills itself or from request parameters, never from path variables.
#[rustfmt::skip]
const NOT_BINDERS: [&str; 52] = [
    "String", "Integer", "Long", "Short", "Byte", "Double", "Float", "Boolean", "Character", "Number", "BigDecimal",
    "BigInteger", "UUID", "Date", "LocalDate", "LocalDateTime", "LocalTime", "Instant", "OffsetDateTime",
    "ZonedDateTime", "Duration", "Model", "ModelMap", "Map", "MultiValueMap", "RedirectAttributes", "BindingResult",
    "Errors", "HttpServletRequest", "HttpServletResponse", "HttpSession", "ServletRequest", "ServletResponse",
    "WebRequest", "NativeWebRequest", "Principal", "Authentication", "Locale", "TimeZone", "ZoneId", "InputStream",
    "OutputStream", "Reader", "Writer", "HttpMethod", "SessionStatus", "UriComponentsBuilder", "ServerWebExchange",
    "Pageable", "Sort", "MultipartFile", "HttpHeaders",
];
const CONFIG_GLOBS: &[&str] = &["**/application*.properties", "**/application*.yml", "**/application*.yaml"];
const VERSIONING_GLOBS: &[&str] = &["**/*.java", "**/*.kt", "**/application*.properties", "**/application*.yml", "**/application*.yaml"];
/// View resolution settings: (prefix key, suffix key, default prefix, default suffix).
const VIEW_SETTINGS: [(&str, &str, &str, &str); 2] = [
    ("spring.mvc.view.prefix", "spring.mvc.view.suffix", "", ".jsp"),
    ("spring.thymeleaf.prefix", "spring.thymeleaf.suffix", "classpath:/templates/", ".html"),
];
/// How long a listing of template files is trusted.
const LISTING_TTL: Duration = Duration::from_secs(60);
const MAX_FILES: u32 = 10_000;

/// A problem of the document and the edit that fixes it.
struct Problem {
    diagnostic: Diagnostic,
    fix: Option<(String, TextEdit)>,
}

/// The names in `{name}`, `{name:regex}` and `{*name}` of a URI template, with
/// the bytes of their braces. `${…}` placeholders are left out.
fn variables(path: &str) -> Vec<(String, usize, usize)> {
    let b = path.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'{' {
            i += 1;
            continue;
        }
        let mut depth = 0;
        let Some(end) = (i..b.len()).find(|&j| {
            depth += match b[j] {
                b'{' => 1,
                b'}' => -1,
                _ => 0,
            };
            depth == 0
        }) else {
            break;
        };
        let name = path[i + 1..end].split(':').next().unwrap_or("").trim_start_matches('*').trim();
        if !(i > 0 && matches!(b[i - 1], b'$' | b'#')) && !name.is_empty() {
            out.push((name.to_owned(), i, end + 1));
        }
        i = end + 1;
    }
    out
}

/// Whether the paths of a mapping annotation are written as literals (not constants or concatenations).
fn literal_paths(a: &Annotation) -> bool {
    ["value", "path"].iter().filter_map(|n| a.arg(n)).all(|arg| {
        java::tokens(&arg.text).iter().all(|t| t.string().is_some() || matches!(t.s, "{" | "}" | ","))
            && !arg.strings.iter().any(|s| s.value.contains("${") || s.value.contains("#{"))
    })
}

/// Whether Spring may fill path variables into this parameter's properties (a
/// form or command object).
fn binds_path_variables(p: &Param) -> bool {
    p.annotation("ModelAttribute").is_some()
        || p.annotations.iter().all(|a| matches!(a.name.as_str(), "Valid" | "Validated"))
            && p.ty.name.starts_with(|c: char| c.is_ascii_uppercase())
            && !NOT_BINDERS.contains(&p.ty.name.as_str())
}

/// `@PathVariable` parameters naming no template variable (errors) and
/// template variables no parameter takes (warnings).
fn path_problems(text: &str, file: &JavaFile) -> Vec<Problem> {
    let mut out = Vec::new();
    for class in file.types.iter().filter(|t| t.has(&["Controller", "RestController"])) {
        if class.annotation("RequestMapping").is_some_and(|a| !literal_paths(a)) {
            continue;
        }
        for m in class.methods() {
            let maps: Vec<&Annotation> = m.annotations.iter().filter(|a| MAPPINGS.contains(&a.name.as_str())).collect();
            if maps.is_empty() || !maps.iter().all(|a| literal_paths(a)) {
                continue;
            }
            let mut paths: Vec<String> = java::mappings(class, m).into_iter().map(|(_, p)| p).collect();
            paths.dedup();
            let in_paths: Vec<Vec<String>> = paths.iter().map(|p| variables(p).into_iter().map(|v| v.0).collect()).collect();
            let (mut claimed, mut bound, mut takes_all) = (Vec::new(), Vec::new(), false);
            for p in &m.params {
                let Some(a) = p.annotation("PathVariable") else {
                    takes_all |= binds_path_variables(p);
                    continue;
                };
                if matches!(p.ty.name.as_str(), "Map" | "MultiValueMap") {
                    takes_all = true;
                    continue;
                }
                let name = match a.args.iter().find(|x| x.name == "value" || x.name == "name") {
                    None => p.name.clone(),
                    Some(arg) if arg.text.starts_with('"') && arg.strings.len() == 1 => arg.strings[0].value.clone(),
                    Some(_) => continue, // a constant
                };
                claimed.push(name.clone());
                if a.arg("required").is_none_or(|r| r.text != "false") {
                    bound.push((a, name));
                }
            }
            let unclaimed: Vec<&String> =
                in_paths.first().into_iter().flatten().filter(|v| !claimed.contains(v) && in_paths.iter().all(|p| p.contains(v))).collect();
            for (a, name) in bound {
                let Some(i) = in_paths.iter().position(|p| !p.contains(&name)) else {
                    continue;
                };
                let message = format!("`{name}` is not a variable of the mapped path `{}`.", paths[i]);
                let fix = match unclaimed.as_slice() {
                    [only] => Some((
                        format!("Bind the `{{{only}}}` variable"),
                        TextEdit { range: range(text, a.name_at, a.end), text: format!("PathVariable(\"{only}\")") },
                    )),
                    _ => None,
                };
                let diagnostic = Diagnostic::new(range(text, a.at, a.end), Severity::Error, message).code("unknown-path-variable");
                out.push(Problem { diagnostic, fix });
            }
            if takes_all {
                continue;
            }
            for s in maps.iter().flat_map(|a| a.strings(&["value", "path"])) {
                for (name, from, to) in variables(&text[s.at..s.end]) {
                    if !claimed.contains(&name) {
                        let message = format!("No @PathVariable parameter takes the `{name}` variable.");
                        let r = range(text, s.at + from, s.at + to);
                        out.push(Problem {
                            diagnostic: Diagnostic::new(r, Severity::Warning, message).code("unbound-path-variable"),
                            fix: None,
                        });
                    }
                }
            }
        }
    }
    out
}

/// The view-name literals of a file's `@Controller` handler methods: `return
/// "name";`, `new ModelAndView("name")` and `setViewName("name")`.
fn view_literals(text: &str, file: &JavaFile) -> Vec<Str> {
    let mut bodies = Vec::new();
    for class in file.types.iter().filter(|t| t.has(&["Controller"]) && !t.has(&["RestController", "ResponseBody"]) && file.from_spring("Controller")) {
        for m in class.methods() {
            if m.annotations.iter().any(|a|(MAPPINGS.contains(&a.name.as_str())||a.name=="ExceptionHandler")&&file.from_spring(&a.name))
                && !m.has(&["ResponseBody"])
                && let Some(body) = m.body
            {
                bodies.push((body, m.ty.as_ref().is_some_and(|t| t.name == "String")));
            }
        }
    }
    if bodies.is_empty() {
        return Vec::new();
    }
    let inside = |at: usize| bodies.iter().find(|((from, to), _)| *from <= at && at < *to);
    let mut out = Vec::new();
    let toks = java::tokens(text);
    for (k, t) in toks.iter().enumerate() {
        if t.s == "return"
            && inside(t.at).is_some_and(|(_, string)| *string)
            && let Some(lit) = toks.get(k + 1)
            && let Some(s) = lit.string()
            // `;` next, or a literal still being typed
            && (toks.get(k + 2).is_some_and(|n| n.s == ";") || lit.s.len() == 1 || !lit.s.ends_with('"'))
        {
            out.push(s);
        }
    }
    for c in java::calls(text) {
        if inside(c.at).is_some()
            && ((c.is_new && c.name == "ModelAndView") || c.name == "setViewName")
            && let Some(s) = c.args.first().and_then(|a| a.string.clone())
        {
            out.push(s);
        }
    }
    out.retain(|s| !["redirect:", "forward:"].iter().any(|p| s.value.starts_with(p)) && !s.value.contains("${"));
    out.sort_by_key(|s| s.at);
    out
}

const WEB_MVC_CONFIGURER: &str = "org.springframework.web.servlet.config.annotation.WebMvcConfigurer";

/// `registry.addViewController("/x").setViewName("home")` in a file importing the exact `WebMvcConfigurer`.
fn config_view_literals(text: &str, file: &JavaFile) -> Vec<Str> {
    let imported = file.imports.iter().any(|i| !i.is_static && i.path == WEB_MVC_CONFIGURER)
        && !file.imports.iter().any(|i| !i.is_static && i.path.ends_with(".WebMvcConfigurer") && i.path != WEB_MVC_CONFIGURER);
    if !imported {
        return Vec::new();
    }
    let calls = java::calls(text);
    calls
        .iter()
        .filter(|c| c.name == "setViewName" && c.chained)
        .filter(|c| calls.iter().any(|p| p.name == "addViewController" && p.end <= c.at && text[p.end..c.at].trim() == "."))
        .filter_map(|c| c.args.first().and_then(|a| a.string.clone()))
        .filter(|s| text.get(s.at..s.end) == Some(s.value.as_str()) && !s.value.contains("${") && !["redirect:", "forward:"].iter().any(|p| s.value.starts_with(p)))
        .collect()
}

fn source_view_literals(path:&str,text:&str,file:&JavaFile)->Vec<Str>{
    if !is_kotlin(path){
        let mut literals=view_literals(text,file);
        literals.extend(config_view_literals(text,file));
        literals.sort_by_key(|s|s.at);
        return literals
    }
    let tokens=java::tokens(text);let calls=source_calls(path,text);let mut output=Vec::new();
    let literal=|token:java::Tok<'_>|token.string().filter(|s|text.get(s.at..s.end)==Some(s.value.as_str())&&!s.value.contains('$'));
    for class in file.types.iter().filter(|c|c.has(&["Controller"])&&!c.has(&["RestController","ResponseBody"])&&file.from_spring("Controller")) {
        for method in class.methods().filter(|m|m.annotations.iter().any(|a|(MAPPINGS.contains(&a.name.as_str())||a.name=="ExceptionHandler")&&file.from_spring(&a.name))) {
            if method.has(&["ResponseBody"]){continue}
            let Some((from,to))=method.body else{continue};let body=tokens.iter().copied().filter(|t|from<=t.at&&t.at<to).collect::<Vec<_>>();
            if body.len()==1 && method.ty.as_ref().is_some_and(|ty|ty.name=="String"||(ty.name=="Unit"&&ty.at==method.at)) {
                if let Some(value)=literal(body[0]){output.push(value);}
            }
            if method.ty.as_ref().is_some_and(|ty|ty.name=="String") {
                let mut depth=0;
                for (index,token) in body.iter().enumerate(){
                    if token.s=="{"{depth+=1;}else if token.s=="}"{depth-=1;}
                    if token.s=="return"&&depth==1 {
                        if let Some(value)=body.get(index+1).copied().and_then(literal) {
                            if body.get(index+2).is_none_or(|next|matches!(next.s,";"|"}")){output.push(value);}
                        }
                    }
                }
            }
            if file.from_spring("ModelAndView") {
                for call in calls.iter().filter(|c|from<=c.at&&c.at<to&&c.is_new&&c.name=="ModelAndView") {
                    let before=tokens.partition_point(|t|t.at<call.at);
                    let returned=call.at==from||before>0&&tokens[before-1].s=="return";
                    if returned {if let Some(value)=call.args.first().and_then(|arg|arg.string.as_ref()).filter(|s|text.get(s.at..s.end)==Some(s.value.as_str())&&!s.value.contains('$')){output.push(value.clone());}}
                }
            }
        }
    }
    output.retain(|s|!["redirect:","forward:"].iter().any(|prefix|s.value.starts_with(prefix)));
    output.sort_by_key(|s|s.at);output.dedup_by_key(|s|s.at);output
}

/// `fragments/header :: header` → `fragments/header`; `/users/list` → `users/list`.
fn view_name(literal: &str) -> &str {
    literal.split("::").next().unwrap_or("").trim().trim_start_matches('/')
}

/// The settings of a configuration file that move templates.
fn view_settings(path: &str, text: &str) -> Option<Vec<(String, String)>> {
    let wanted: Vec<&str> = VIEW_SETTINGS.iter().flat_map(|s| [s.0, s.1]).collect();
    let found: Vec<(String, String)> = config::keys(path, text)
        .into_iter()
        .map(|k| (config::canonical(&k.key), k.value))
        .filter(|(k, _)| wanted.contains(&k.as_str()))
        .collect();
    (!found.is_empty()).then_some(found)
}

/// Where templates live: a directory under a module root and the extensions found there.
fn template_roots(settings: &BTreeMap<String, String>) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = vec![
        ("src/main/resources/templates/".into(), TEMPLATE_EXTENSIONS.iter().map(|e| e.to_string()).collect()),
        ("src/main/webapp/WEB-INF/views/".into(), vec!["jsp".into()]),
        ("src/main/webapp/WEB-INF/jsp/".into(), vec!["jsp".into()]),
    ];
    for (prefix_key, suffix_key, default_prefix, default_suffix) in VIEW_SETTINGS {
        let (prefix, suffix) = (settings.get(prefix_key), settings.get(suffix_key));
        if prefix.is_none() && suffix.is_none() {
            continue;
        }
        let prefix = prefix.map_or(default_prefix, |p| p.as_str());
        let dir = match prefix.strip_prefix("classpath:") {
            Some(p) => format!("src/main/resources/{}", p.trim_matches('/')),
            None if prefix.contains(':') || prefix.trim_matches('/').is_empty() => continue,
            None => format!("src/main/webapp/{}", prefix.trim_matches('/')),
        };
        let ext = suffix.map_or(default_suffix, |s| s.as_str()).trim_start_matches('.');
        if !ext.is_empty() {
            out.push((format!("{dir}/"), vec![ext.to_owned()]));
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Template {
    /// The view name that resolves to it.
    name: String,
    path: String,
}

impl Template {
    fn location(&self) -> Location {
        Location { path: self.path.clone(), range: Range::default() }
    }

    /// `templates/users/list.html`.
    fn short(&self) -> &str {
        self.path.split_once("/src/main/").map_or(&self.path, |(_, rest)| rest)
    }
}

/// A span of the document leading to the code it stands for.
struct Link {
    at: usize,
    end: usize,
    tooltip: String,
    targets: Vec<Location>,
}

/// The methods a handler reference names: `this::` in the file, `Type::` anywhere.
fn handler_targets(index: &Index<FileInfo>, path: &str, handler: &Handler) -> Vec<Location> {
    index
        .iter()
        .filter(|(p, _)| handler.ty.is_some() || *p == path)
        .flat_map(|(p, info)| {
            info.methods
                .iter()
                .filter(|m| m.name == handler.method && handler.ty.as_ref().is_none_or(|t| *t == m.ty))
                .map(move |m| Location { path: p.to_owned(), range: m.range })
        })
        .collect()
}

pub struct WebTools {
    endpoints: Indexes<FileInfo>,
    models: Indexes<Vec<model_links::Fact>>,
    settings: Indexes<Vec<(String, String)>>,
    versioning: Indexes<checks::Versioning>,
    routes: Option<RouteList>,
    /// Glob → when listed and the files.
    listings: BTreeMap<String, (Instant, Vec<String>)>,
}

impl Default for WebTools {
    fn default() -> Self {
        Self {
            endpoints: Indexes::new(SOURCE_GLOBS, endpoints::file_info),
            models: Indexes::new(SOURCE_GLOBS, model_links::facts),
            settings: Indexes::new(CONFIG_GLOBS, view_settings),
            versioning: Indexes::new(VERSIONING_GLOBS, checks::versioning),
            routes: None,
            listings: BTreeMap::new(),
        }
    }
}

impl WebTools {
    /// Reads on from the project's files for at most `budget`, so the providers can stay on cached facts.
    pub fn live_warm(&mut self, project: &Project, budget: Duration, discover: bool) {
        if budget.is_zero() {
            return;
        }
        self.endpoints.warm(project, budget / 4, discover);
        self.models.warm(project, budget / 4, discover);
        self.settings.warm(project, budget / 4, discover);
        self.versioning.warm(project, budget / 4, discover);
    }

    /// The findings of the text checks over a Java document.
    #[cfg(test)]
    fn checked(&mut self, doc: &Document) -> Vec<Diagnostic> {
        let groups = checks::Groups::from_settings();
        let (configured, standard_parser) = match self.versioning.for_cached_document(&doc.path, &doc.text) {
            Some(index) if groups.api_versioning || groups.version_format => {
                let files: Vec<_> = index.iter().map(|(_, v)| *v).collect();
                (checks::project_versioning(index.complete(), &files), checks::project_standard_parser(index.complete(), &files))
            }
            _ => (None, None),
        };
        checks::check_java(&parse_source(&doc.path, &doc.text), configured, standard_parser, groups)
            .iter()
            .map(|f| f.diagnostic(&doc.text))
            .collect()
    }

    /// The project's templates, those of the document's module first.
    fn templates(&mut self, doc: &Document) -> Vec<Template> {
        let Some(project) = project_of(&doc.path) else {
            return Vec::new();
        };
        let mut settings = BTreeMap::new();
        if let Some(index) = self.settings.for_cached_document(&doc.path, &doc.text) {
            for (key, value) in index.iter().flat_map(|(_, kv)| kv) {
                settings.entry(key.clone()).or_insert_with(|| value.clone());
            }
        }
        let root = project.root().unwrap_or_default();
        let mut out = Vec::new();
        for (dir, extensions) in template_roots(&settings) {
            for ext in extensions {
                let glob = format!("**/{dir}**/*.{ext}");
                let key = format!("{root}\n{glob}");
                if self.listings.get(&key).is_none_or(|(at, _)| at.elapsed() > LISTING_TTL) {
                    let files = project.find_files(&glob, MAX_FILES).unwrap_or_default();
                    self.listings.insert(key.clone(), (Instant::now(), files));
                }
                for path in &self.listings[&key].1 {
                    let Some(i) = path.find(&format!("/{dir}")) else {
                        continue;
                    };
                    let Some(name) = path[i + 1 + dir.len()..].strip_suffix(&format!(".{ext}")) else {
                        continue;
                    };
                    if !out.iter().any(|t: &Template| t.path == *path) {
                        out.push(Template { name: name.to_owned(), path: path.clone() });
                    }
                }
            }
        }
        let module = doc.path.split_once("/src/main/").map(|(m, _)| format!("{m}/"));
        out.sort_by_key(|t| !module.as_ref().is_some_and(|m| t.path.starts_with(m)));
        out
    }

    /// The spans of the document that lead to a handler: functional routes' method references and test URLs.
    fn links(&mut self, doc: &Document) -> Vec<Link> {
        let Some(index) = self.endpoints.for_cached_document(&doc.path, &doc.text) else {
            return Vec::new();
        };
        if !index.walked() {
            return Vec::new();
        }
        let index = &*index;
        let mine = index.files().get(&doc.path);
        let mut out = Vec::new();
        for r in endpoints::requests(&doc.path, &doc.text) {
            let targets: Vec<Location> = index
                .iter()
                .flat_map(|(p, info)| {
                    info.endpoints.iter().filter(|e| endpoints::serves(e, &r)).map(move |e| Location { path: p.to_owned(), range: e.range })
                })
                .collect();
            if !targets.is_empty() {
                out.push(Link { at: r.at, end: r.end, tooltip: format!("Handler of {} {}", r.verb, r.path), targets });
            }
        }
        for e in mine.into_iter().flat_map(|info| &info.endpoints) {
            let Some(handler) = &e.handler else { continue };
            let targets = handler_targets(index, &doc.path, handler);
            if !targets.is_empty() {
                out.push(Link { at: handler.at, end: handler.end, tooltip: format!("Handler of {} {}", e.verb, e.path), targets });
            }
        }
        out
    }

    /// The view literals of the document and the templates each names.
    fn views(&mut self, doc: &Document) -> Vec<(Str, Vec<Template>)> {
        let literals = if is_xml(doc) { xml_checks::view_names(&doc.text) } else { source_view_literals(&doc.path,&doc.text,&parse_source(&doc.path,&doc.text)) };
        if literals.is_empty() {
            return Vec::new();
        }
        let templates = self.templates(doc);
        literals
            .into_iter()
            .map(|s| {
                let found: Vec<Template> = templates.iter().filter(|t| t.name == view_name(&s.value)).cloned().collect();
                (s, found)
            })
            .collect()
    }
}

/// XML configuration is not in the versioning inventory: every XML file must be readable and name no view resolver.
#[cfg(test)]
fn xml_has_default_views(doc: &Document) -> bool {
    let Some(project) = project_of(&doc.path) else { return false };
    let Ok(files) = project.find_files("**/*.xml", 200) else { return false };
    let resolver = |text: &str| checks::VIEW_RESOLVER_MARKERS.iter().any(|m| text.contains(m)) || text.contains("view-resolvers") || text.contains("InternalResourceViewResolver");
    !resolver(&doc.text) && files.len() < 200 && files.iter().filter(|f| **f != doc.path).all(|f| project.read_file(f).is_ok_and(|t| !resolver(&t)))
}

impl WebTools {
    /// Hints for view names no template answers, once the project has templates, every source was read and none brings a view resolver of its own.
    #[cfg(test)]
    fn unresolved_views(&mut self, doc: &Document) -> Vec<Diagnostic> {
        if !host::setting_bool("springWeb.reportUnresolvedViews").unwrap_or(true) {
            return Vec::new();
        }
        let Some(index) = self.versioning.for_cached_document(&doc.path, &doc.text) else {
            return Vec::new();
        };
        let files: Vec<_> = index.iter().map(|(_, v)| *v).collect();
        if !checks::project_has_default_views(index.complete(), &files) {
            return Vec::new();
        }
        if is_xml(doc) && !xml_has_default_views(doc) {
            return Vec::new();
        }
        let views = self.views(doc);
        if views.is_empty() || self.templates(doc).is_empty() {
            return Vec::new();
        }
        views
            .into_iter()
            .filter(|(s, found)| found.is_empty() && !view_name(&s.value).is_empty())
            .map(|(s, _)| {
                let message = format!(
                    "No template named \"{}\" in the known template folders; another view resolver may serve it.",
                    view_name(&s.value)
                );
                Diagnostic::new(range(&doc.text, s.at, s.end), Severity::Hint, message).code("unresolved-view")
            })
            .collect()
    }

    /// `apiVersion(...)` in a test of a project with nothing that carries the version into a request.
    #[cfg(test)]
    fn api_version_hints(&mut self, doc: &Document) -> Vec<Diagnostic> {
        if !doc.path.contains("/src/test/") || !doc.text.contains("apiVersion(") {
            return Vec::new();
        }
        let Some(index) = self.versioning.for_cached_document(&doc.path, &doc.text) else {
            return Vec::new();
        };
        let files: Vec<_> = index.iter().map(|(_, v)| *v).collect();
        if !index.complete() || files.iter().any(|f| f.inserter || (f.is_config_file && f.configured)) {
            return Vec::new();
        }
        source_calls(&doc.path, &doc.text)
            .into_iter()
            .filter(|c| c.name == "apiVersion" && !c.is_new)
            .map(|c| {
                let message = "The project defines no ApiVersionInserter and no spring.mvc.apiversion.* configuration, so this version may not reach the request.";
                Diagnostic::new(range(&doc.text, c.at, c.at + c.name.len()), Severity::Hint, message).code("api-version-without-inserter")
            })
            .collect()
    }

    /// One marker per endpoint of the document showing its URL, on lines no link marker already uses.
    fn url_markers(&mut self, doc: &Document, taken: &BTreeSet<u32>) -> Vec<LineMarker> {
        if !host::setting_bool("springWeb.showUrlMarkers").unwrap_or(false) {
            return Vec::new();
        }
        let Some(index) = self.endpoints.for_cached_document(&doc.path, &doc.text) else {
            return Vec::new();
        };
        let Some(mine) = index.files().get(&doc.path) else {
            return Vec::new();
        };
        let mut lines = taken.clone();
        let mut out = Vec::new();
        for e in &mine.endpoints {
            let line = e.range.start.line;
            if !lines.insert(line) {
                continue;
            }
            let verb = if e.verb.is_empty() { "ANY" } else { e.verb.as_str() };
            out.push(LineMarker { line, icon: Some(ROUTE_MARKER.into()), tooltip: format!("URL: {verb} {}", e.path), target: None });
        }
        out
    }

    fn request_completions(&mut self, doc: &Document, offset: usize) -> Option<Vec<CompletionItem>> {
        let request = endpoints::typed_requests(&doc.path, &doc.text).into_iter().find(|r| r.at <= offset && offset <= r.end)?;
        let http=doc.language=="http" || doc.path.ends_with(".http") || doc.path.ends_with(".rest");
        // The native HTTP editor owns ordinary endpoint completion through
        // EndpointsProvider. Only supply its missing HEAD-to-GET fallback.
        if http && request.verb!="HEAD" {return Some(Vec::new())}
        let index = self.endpoints.for_cached_document(&doc.path, &doc.text)?;
        let native_paths=index.iter().flat_map(|(_,info)|&info.endpoints).filter(|e|e.verb.is_empty()||e.verb=="HEAD").map(|e|e.path.as_str()).collect::<BTreeSet<_>>();
        let replace = range(&doc.text, request.at, request.end);
        let mut seen = BTreeSet::new();
        let mut items = Vec::new();
        for (_, info) in index.iter() {
            for e in info.endpoints.iter().filter(|e| if http {e.verb=="GET"&&!native_paths.contains(e.path.as_str())}else{e.verb.is_empty() || e.verb == request.verb || (request.verb == "HEAD" && e.verb == "GET")}) {
                if seen.insert(e.path.clone()) {
                    let verb = if e.verb.is_empty() { "ANY" } else { e.verb.as_str() };
                    items.push(CompletionItem {
                        detail: Some(verb.to_owned()),
                        replace: Some(replace),
                        ..CompletionItem::new(&if http {http_path_variables(&e.path)}else{e.path.clone()}, CompletionKind::Value)
                    });
                }
            }
        }
        items.sort_by(|a, b| a.label.cmp(&b.label));
        Some(items)
    }

    fn template_completions(&mut self, doc: &Document, at: usize) -> Vec<CompletionItem> {
        let Some(slot) = template_links::completion(&doc.text, at) else {
            let Some((_,replace))=model_links::reference(&doc.text,at) else{return Vec::new()};
            let mut items=self.model_candidates(doc).into_iter().map(|(name,location,session)|CompletionItem{detail:Some(format!("{} · {}",if session{"Session attribute"}else{"MVC model"},location.path)),replace:Some(replace),..CompletionItem::new(&name,CompletionKind::Variable)}).collect::<Vec<_>>();
            items.sort_by(|a,b|a.label.cmp(&b.label));items.dedup_by(|a,b|a.label==b.label);return items
        };
        match slot {
            template_links::Slot::Template(replace) => self.templates(doc).into_iter().map(|t| CompletionItem {
                detail:Some(t.short().into()), replace:Some(replace), ..CompletionItem::new(&t.name,CompletionKind::File)
            }).collect(),
            template_links::Slot::Fragment { template, replace } => {
                let Some(project)=project_of(&doc.path) else {return Vec::new()};
                let sources=if template.is_empty()||template=="this" {vec![doc.text.clone()]} else {
                    self.templates(doc).into_iter().filter(|t|t.name==template).filter_map(|t|project.read_file(&t.path).ok()).collect()
                };
                let mut names=sources.iter().flat_map(|s|template_links::fragment_names(s)).collect::<Vec<_>>();names.sort();names.dedup();
                names.into_iter().map(|name|CompletionItem{detail:Some("Thymeleaf fragment".into()),replace:Some(replace),..CompletionItem::new(&name,CompletionKind::Function)}).collect()
            }
        }
    }

    fn model_candidates(&mut self,doc:&Document)->Vec<(String,Location,bool)>{
        let mut views=self.templates(doc).into_iter().filter(|t|t.path==doc.path).map(|t|t.name).collect::<Vec<_>>();
        if let Some((_,path))=doc.path.split_once("/src/main/resources/templates/") {if let Some((name,_))=path.rsplit_once('.') {views.push(name.into());}}
        let Some(index)=self.models.for_cached_document(&doc.path,&doc.text) else{return Vec::new()};
        index.iter().flat_map(|(path,facts)|facts.iter().filter(|f|f.view==model_links::ALL_VIEWS||views.contains(&f.view)).map(move|f|(f.name.clone(),Location{path:path.into(),range:f.range},f.session))).take(200).collect()
    }
    fn model_targets(&mut self,doc:&Document,at:usize)->Vec<Location>{
        let Some((name,_))=model_links::reference(&doc.text,at) else{return Vec::new()};
        self.model_candidates(doc).into_iter().filter(|(candidate,..)|*candidate==name).map(|(_,location,_)|location).collect()
    }

    fn template_targets(&mut self, doc: &Document, at: usize) -> Vec<Location> {
        let Some(reference) = template_links::reference(&doc.text, at) else { return Vec::new() };
        let Some(project) = project_of(&doc.path) else { return Vec::new() };
        let paths = if reference.template.is_empty() || reference.template == "this" {
            vec![doc.path.clone()]
        } else {
            self.templates(doc).into_iter().filter(|t| t.name == reference.template).map(|t| t.path).collect()
        };
        let mut targets = Vec::new();
        for path in paths {
            if reference.fragment.is_none() && reference.element.is_none() { targets.push(Location { path, range: Range::default() }); continue }
            let source = if path == doc.path { doc.text.clone() } else {
                let Ok(source) = project.read_file(&path) else { continue }; source
            };
            if let Some((tag, id)) = &reference.element {
                let ids = template_links::element_ids(&source, tag.as_deref(), id);
                if let [range] = ids[..] { targets.push(Location { path, range }); }
                continue;
            }
            let Some(fragment) = &reference.fragment else { continue };
            targets.extend(template_links::declarations(&source, fragment).into_iter().map(|range| Location { path:path.clone(), range }));
        }
        targets
    }

    /// The files of the static folders a string literal under the caret names.
    fn static_files(&self, doc: &Document, at: usize) -> Vec<Location> {
        let Some(project) = project_of(&doc.path) else {
            return Vec::new();
        };
        let literal = if matches!(doc.language.as_str(), "html" | "jsp") {
            spring_text::xml::elements(&doc.text).into_iter().find_map(|e| {
                let url_tag = matches!(e.name.as_str(), "c:url" | "spring:url");
                e.attrs.into_iter().find(|a| a.holds(at) && (matches!(a.name.as_str(), "src" | "href" | "th:src" | "th:href") || (url_tag && a.name == "value"))).map(|a| a.value)
            })
        } else {
            java::tokens(&doc.text).into_iter().filter_map(|t| t.string()).find(|s| s.holds(at)).map(|s| s.value)
        };
        let Some(literal) = literal else { return Vec::new() };
        let literal = literal.strip_prefix("@{").and_then(|s| s.strip_suffix('}')).unwrap_or(&literal);
        let Some(rel) = static_resource(literal) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for root in STATIC_ROOTS {
            for path in project.find_files(&format!("**/{root}/{rel}"), 20).unwrap_or_default() {
                if !found.contains(&path) {
                    found.push(path);
                }
            }
        }
        found.into_iter().map(|path| Location { path, range: Range::default() }).collect()
    }
}

fn problems(doc: &Document) -> Vec<Problem> {
    if is_xml(doc) {
        return Vec::new();
    }
    path_problems(&doc.text, &parse_source(&doc.path, &doc.text))
}

fn is_xml(doc: &Document) -> bool {
    doc.language == "xml" || doc.path.ends_with(".xml")
}

/// Live templates: abbreviation, annotation line and the label of the mapping.
const MAPPING_SNIPPETS: [(&str, &str, &str); 6] = [
    ("getm", "@GetMapping(\"${1:/}\")", "get"),
    ("postm", "@PostMapping(\"${1:/}\")", "post"),
    ("putm", "@PutMapping(\"${1:/}\")", "put"),
    ("delm", "@DeleteMapping(\"${1:/}\")", "delete"),
    ("patm", "@PatchMapping(\"${1:/}\")", "patch"),
    ("reqm", "@RequestMapping(value = \"${1:/}\", method = RequestMethod.${2:GET})", "handle"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    TopLevel,
    Controller,
    Class,
}

/// Abbreviation, scope, detail, Java body and Kotlin body of the structure snippets.
const STRUCTURE_SNIPPETS: [(&str, Scope, &str, &str, &str); 6] = [
    (
        "restc",
        Scope::TopLevel,
        "Spring REST controller",
        "@RestController\n@RequestMapping(\"${1:/}\")\npublic class ${2:Name}Controller {\n    $0\n}",
        "@RestController\n@RequestMapping(\"${1:/}\")\nclass ${2:Name}Controller {\n    $0\n}",
    ),
    (
        "ctrl",
        Scope::TopLevel,
        "Spring controller",
        "@Controller\n@RequestMapping(\"${1:/}\")\npublic class ${2:Name}Controller {\n    $0\n}",
        "@Controller\n@RequestMapping(\"${1:/}\")\nclass ${2:Name}Controller {\n    $0\n}",
    ),
    (
        "advice",
        Scope::TopLevel,
        "Spring controller advice",
        "@RestControllerAdvice\npublic class ${1:Global}ExceptionHandler {\n    $0\n}",
        "@RestControllerAdvice\nclass ${1:Global}ExceptionHandler {\n    $0\n}",
    ),
    (
        "exh",
        Scope::Controller,
        "Spring exception handler",
        "@ExceptionHandler(${1:Exception}.class)\npublic ${2:ResponseEntity<?>} ${3:handle}(${1:Exception} ${4:e}) {\n    $0\n}",
        "@ExceptionHandler(${1:Exception}::class)\nfun ${3:handle}(${4:e}: ${1:Exception}) {\n    $0\n}",
    ),
    (
        "initb",
        Scope::Controller,
        "Spring init binder",
        "@InitBinder\npublic void ${1:initBinder}(WebDataBinder binder) {\n    $0\n}",
        "@InitBinder\nfun ${1:initBinder}(binder: WebDataBinder) {\n    $0\n}",
    ),
    (
        "router",
        Scope::Class,
        "Spring functional router bean",
        "@Bean\npublic RouterFunction<ServerResponse> ${1:routes}(${2:Handler handler}) {\n    return RouterFunctions.route()\n        .GET(\"${3:/}\", handler::${4:list})\n        .build();\n}",
        "@Bean\nfun ${1:routes}(${2:handler: Handler}) = router {\n    GET(\"${3:/}\", handler::${4:list})\n}",
    ),
];

fn word_before(text: &str, offset: usize) -> &str {
    text[..offset].rsplit(|c: char| !c.is_alphanumeric()).next().unwrap_or("")
}

/// The snippets whose abbreviation starts with the word before `offset`: mapping methods and handlers
/// in a controller body, routers in any class body, controller classes outside every type.
fn mapping_snippets(doc: &Document, offset: usize) -> Vec<CompletionItem> {
    let word = word_before(&doc.text, offset);
    if word.is_empty() {
        return Vec::new();
    }
    let file = parse_source(&doc.path, &doc.text);
    let in_class_body = |t: &spring_text::java::TypeDecl| {
        t.start < offset && offset < t.end && !t.methods().any(|m| m.body.is_some_and(|(from, to)| from < offset && offset < to))
    };
    let in_controller = file.types.iter().any(|t| t.has(&["Controller", "RestController"]) && in_class_body(t));
    let in_class = file.types.iter().any(|t| in_class_body(t));
    let top_level = !file.types.iter().any(|t| t.start < offset && offset < t.end);
    let kotlin = is_kotlin(&doc.path);
    let mut items = Vec::new();
    if in_controller {
        for (abbreviation, annotation, name) in MAPPING_SNIPPETS.iter().filter(|(a, _, _)| a.starts_with(word)) {
            let handler = if kotlin {
                format!("fun ${{3:{name}}}(${{4}}) {{\n    $0\n}}")
            } else {
                format!("public ${{3:ResponseEntity<?>}} ${{4:{name}}}(${{5}}) {{\n    $0\n}}")
            };
            items.push(CompletionItem {
                detail: Some("Spring request mapping".into()),
                insert_text: format!("{annotation}\n{handler}"),
                snippet: true,
                ..CompletionItem::new(abbreviation, CompletionKind::Snippet)
            });
        }
    }
    for (abbreviation, scope, detail, java, kotlin_body) in STRUCTURE_SNIPPETS.iter().filter(|(a, ..)| a.starts_with(word)) {
        let fits = match scope {
            Scope::TopLevel => top_level,
            Scope::Controller => in_controller,
            Scope::Class => in_class,
        };
        if fits {
            items.push(CompletionItem {
                detail: Some((*detail).into()),
                insert_text: (if kotlin { kotlin_body } else { java }).to_string(),
                snippet: true,
                ..CompletionItem::new(abbreviation, CompletionKind::Snippet)
            });
        }
    }
    items
}

const STATIC_EXTENSIONS: [&str; 13] = ["css", "js", "mjs", "map", "png", "jpg", "jpeg", "gif", "svg", "ico", "webp", "woff2", "html"];
const STATIC_ROOTS: [&str; 3] = ["static", "public", "META-INF/resources"];

/// The path under a static root that a string literal names: `/css/app.css`, `classpath:/static/js/a.js`.
fn static_resource(literal: &str) -> Option<&str> {
    let rel = literal.strip_prefix("classpath:").unwrap_or(literal).trim_start_matches('/');
    let rel = rel.strip_prefix("static/").or_else(|| rel.strip_prefix("public/")).unwrap_or(rel);
    let rel = rel.split(['?', '#']).next()?;
    let extension = rel.rsplit_once('.')?.1;
    let plain = !rel.is_empty() && !rel.contains(['*', '{', '$', ':', ' ', '"', '[', ']', '\\'])
        && !rel.split('/').any(|part| matches!(part, "." | ".."));
    (plain && STATIC_EXTENSIONS.contains(&extension)).then_some(rel)
}

/// Most markers one line gets; the rest are named in the tooltip.
const MAX_LINE_TARGETS: usize = 200;

/// One marker per target, all on `line` with `icon`: the IDE folds them into one gutter button
/// that jumps to a single target or opens its chooser for several.
fn per_target(line: u32, icon: &str, targets: Vec<(Location, String)>) -> Vec<LineMarker> {
    let total = targets.len();
    let note = if total > MAX_LINE_TARGETS { format!(" (first {MAX_LINE_TARGETS} of {total})") } else { String::new() };
    targets
        .into_iter()
        .take(MAX_LINE_TARGETS)
        .map(|(target, tooltip)| LineMarker { line, icon: Some(icon.into()), tooltip: format!("{tooltip}{note}"), target: Some(target) })
        .collect()
}

impl WebTools {
    fn editor_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let links = self.links(doc).into_iter().flat_map(|l| {
            let targets = l.targets.into_iter().map(|t| (t, l.tooltip.clone())).collect();
            per_target(position(&doc.text, l.at).line, ROUTE_MARKER, targets)
        });
        let views = self.views(doc).into_iter().flat_map(|(s, found)| {
            let targets = found.iter().map(|t| (t.location(), format!("View template {}", t.short()))).collect();
            per_target(position(&doc.text, s.at).line, TEMPLATE_MARKER, targets)
        });
        let mut markers: Vec<LineMarker> = views.chain(links).collect();
        let taken = markers.iter().map(|m| m.line).collect();
        markers.extend(self.url_markers(doc, &taken));
        markers
    }
}

impl Providers for WebTools {
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        if is_xml(doc) {
            return Vec::new();
        }
        self.editor_markers(doc)
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        if is_xml(doc){
            let mut found = xml_checks::definition(doc,at);
            if let Some(at) = offset(&doc.text, at) {
                found.extend(self.views(doc).into_iter().filter(|(s, _)| s.holds(at)).flat_map(|(_, f)| f.into_iter().map(|t| t.location())));
            }
            return found;
        }
        let Some(at) = offset(&doc.text, at) else {
            return Vec::new();
        };
        if matches!(doc.language.as_str(), "html" | "jsp") {
            let mut found = self.template_targets(doc, at);
            found.extend(self.model_targets(doc,at));
            return if found.is_empty() { self.static_files(doc, at) } else { found };
        }
        let mut out: Vec<Location> = self
            .views(doc)
            .into_iter()
            .filter(|(s, _)| s.holds(at))
            .flat_map(|(_, found)| found.into_iter().map(|t| t.location()))
            .collect();
        out.extend(self.links(doc).into_iter().filter(|l| l.at <= at && at <= l.end).flat_map(|l| l.targets));
        if out.is_empty() && !is_xml(doc) {
            out = self.static_files(doc, at);
        }
        out
    }

    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(offset) = offset(&doc.text, at) else {
            return Vec::new();
        };
        if matches!(doc.language.as_str(),"html"|"jsp") { return self.template_completions(doc,offset); }
        if is_xml(doc) {
            return xml_checks::completion(doc,at);
        }
        if let Some(items) = self.request_completions(doc, offset) {
            return items;
        }
        let snippets = mapping_snippets(doc, offset);
        if !snippets.is_empty() {
            return snippets;
        }
        let literals = source_view_literals(&doc.path,&doc.text,&parse_source(&doc.path,&doc.text));
        let Some(s) = literals.iter().find(|s| s.holds(offset)) else {
            return Vec::new();
        };
        let replace = range(&doc.text, s.at, s.end);
        let mut seen = BTreeSet::new();
        self.templates(doc)
            .into_iter()
            .filter(|t| seen.insert(t.name.clone()))
            .map(|t| CompletionItem {
                detail: Some(t.short().to_owned()),
                replace: Some(replace),
                ..CompletionItem::new(&t.name, CompletionKind::File)
            })
            .collect()
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        let mut actions: Vec<CodeAction> = problems(doc)
            .into_iter()
            .filter(|p| contains(p.diagnostic.range, selection.start))
            .filter_map(|p| p.fix)
            .map(|(title, edit)| CodeAction {
                title,
                kind: Some("quickfix".into()),
                preferred: true,
                edits: vec![(doc.path.clone(), vec![edit])],
                data: None,
            })
            .collect();
        actions.extend(generate::actions(&doc.path, &doc.text, selection));
        actions
    }
}

const NO_ENDPOINTS: &str = "No Spring endpoints found. Add a @RestController or @Controller with @GetMapping, @PostMapping or @RequestMapping methods.";

fn no_routes_error(scanning: bool) -> Error {
    if scanning {
        return "Still indexing endpoints. Try again in a moment.".into();
    }
    NO_ENDPOINTS.into()
}

const BUILD_FILES: [&str; 5] = ["pom.xml", "build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"];

/// A controller method mapping (as opposed to a functional route): it sits on a method name.
fn annotated(info: &endpoints::FileInfo, e: &endpoints::Endpoint) -> bool {
    e.handler.is_none() && info.methods.iter().any(|m| m.range == e.range)
}

struct Routes {
    routes: Vec<Route>,
    scanning: bool,
    walked: bool,
    note: Option<String>,
}

impl WebTools {
    fn route_list(&mut self) -> &mut RouteList {
        self.routes.get_or_insert_with(|| RouteList::new("spring.routes", "aritti.spring-web"))
    }

    fn pick_route(&mut self, ctx: &Invocation) -> Result<Option<Route>, Error> {
        let project = ctx.project().ok_or("Open a project to choose an endpoint.")?;
        let Routes { routes, scanning, .. } = self.routes(&project, COMMAND_BUDGET);
        if routes.is_empty() {
            return Err(no_routes_error(scanning));
        }
        let items: Vec<_> = routes
            .iter()
            .enumerate()
            .map(|(at, route)| openidea_plugin::PickItem::new(&at.to_string(), &format!("{} {}", route.method, route.path)).description(&route.handler.class))
            .collect();
        let Some(chosen) = host::pick("Choose an endpoint", &items)? else {
            return Ok(None);
        };
        Ok(chosen.parse::<usize>().ok().and_then(|at| routes.into_iter().nth(at)))
    }

    fn routes(&mut self, project: &Project, budget: Duration) -> Routes {
        self.routes_where(project, budget, |_, _| true)
    }

    /// The routes to publish to the IDE. It already indexes @Controller/@RestController
    /// mappings itself and draws its own gutter icon on them; publishing those too gave
    /// every mapping a second icon and duplicate search results. Functional routes stay.
    pub(crate) fn ide_routes(&mut self, project: &Project, budget: Duration) -> Routes {
        self.routes_where(project, budget, |info, e| !annotated(info, e))
    }

    fn routes_where(&mut self, project: &Project, budget: Duration, keep: impl Fn(&endpoints::FileInfo, &endpoints::Endpoint) -> bool) -> Routes {
        let index = self.endpoints.get(project, budget);
        let (scanning, walked, note) = (index.scanning(), index.walked(), spring_text::index::coverage_note(index));
        let mut routes = Vec::new();
        for (path, info) in index.iter() {
            for e in info.endpoints.iter().filter(|e| keep(info, e)) {
                let class = info
                    .methods
                    .iter()
                    .find(|m| m.range == e.range)
                    .map(|m| m.ty.clone())
                    .or_else(|| e.handler.as_ref().and_then(|h| h.ty.clone()))
                    .unwrap_or_else(|| path.rsplit(['/', '\\']).next().unwrap_or(path).split('.').next().unwrap_or_default().to_owned());
                routes.push(Route {
                    method: if e.verb.is_empty() { "ANY".into() } else { e.verb.clone() },
                    path: e.path.clone(),
                    handler: RouteHandler {
                        class,
                        member: e.handler.as_ref().map(|h| h.method.clone()).or_else(|| {
                            info.methods.iter().find(|m| m.range == e.range).map(|m| m.name.clone())
                        }),
                    },
                    name: e.name.clone(),
                    location: Some(Location { path: path.to_owned(), range: e.range }),
                });
            }
        }
        routes.sort_by(|a, b| (&a.path, &a.method, &a.handler.class).cmp(&(&b.path, &b.method, &b.handler.class)));
        Routes { routes, scanning, walked, note }
    }

    fn endpoints_view(&mut self, view: &str, project: &Project, budget: Duration) -> View {
        let Routes { routes, scanning, note, .. } = self.routes(project, budget);
        if scanning {
            refresh(view);
        }

        let refresh = Button::new("refresh", "Refresh").icon("refresh");
        if routes.is_empty() {
            let text = if !scanning && !BUILD_FILES.iter().any(|file| project.exists(file).unwrap_or(false)) {
                "No Spring endpoints found. This project doesn't look like a Spring project: it has no pom.xml or build.gradle.".to_owned()
            } else {
                spring_text::index::empty_message(NO_ENDPOINTS.trim_end_matches('.'), scanning, note.as_deref())
            };
            return View::message(&text).button(refresh);
        }

        let list = self.route_list();
        list.set_routes(routes);
        let mut view = list.render(View::new(Body::List(Vec::new())).button(refresh));
        if let Some(note) = note {
            view.status = view.status.map(|status| format!("{status} · {note}"));
        }
        view
    }
}

const DEFAULT_BASE_URL: &str = "http://localhost:8080";
fn http_path_variables(path:&str)->String{
    let mut result=path.to_owned();
    for (name,start,end) in variables(path).into_iter().rev(){
        if &path[start+1..end-1]==name && name.bytes().all(|c|c.is_ascii_alphanumeric()||matches!(c,b'_'|b'.'|b'-')){result.replace_range(start..end,&format!("{{{{{name}}}}}"));}
    }
    result
}
fn safe_request_path(path:&str)->bool{path.starts_with('/')&&!path.starts_with("//")&&!path.chars().any(|c|c.is_whitespace()||c.is_control())&&!path.contains('\\')}
fn select_http_method(route:&mut Route)->Result<bool,Error>{
    if !safe_request_path(&route.path){return Err(Error::Invalid("Endpoint path contains unsupported whitespace, controls or URL authority.".into()))}
    if route.method!="ANY" {return Ok(true)}
    let Some(method)=host::input("HTTP method for unrestricted mapping",Some("GET"))? else{return Ok(false)};
    let method=method.trim().to_ascii_uppercase();
    if !["GET","POST","PUT","PATCH","DELETE","HEAD","OPTIONS","TRACE","CONNECT"].contains(&method.as_str()) {return Err(Error::Invalid("Choose a standard HTTP method.".into()))}
    route.method=method;Ok(true)
}


impl Views for WebTools {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        (|| -> Result<View, Error> {
            let Some(project) = ctx.project() else {
                return Ok(View::message("Open a project to see its endpoints."));
            };

            Ok(self.endpoints_view(view, &project, VIEW_BUDGET))
        })().map(ui_kit::actions::available)
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if ui_kit::web::handle(self, view, &event, ctx, include_str!("ui_config.json"))? {
            return Ok(());
        }

        if let ViewEvent::Button(b) = &event
            && b == "refresh"
        {
            if let Some(project) = ctx.project()
                && !self.endpoints.get(&project, Duration::ZERO).scanning()
            {
                self.endpoints.reset();
            }
            refresh(view);
            return Ok(());
        }

        match self.route_list().handle(&event) {
            RouteEvent::Changed => refresh(view),
            RouteEvent::Open(route) => {
                if let Some(location) = route.location {
                    let start = location.range.start;
                    host::open_location(&location.path, start.line, start.character)?;
                }
            }
            RouteEvent::Ignored => {}
        }

        Ok(())
    }
}

impl Plugin for WebTools {
    fn run_command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        if id == "create-source-feign-http"{source_lenses::run_feign(ctx,arg)?;return Ok(None)}
        if id == source_lenses::COMMAND { source_lenses::run(ctx,arg)?; return Ok(None); }
        if id == "copy-endpoints-openapi" {
            let project = ctx.project().ok_or_else(|| Error::Failed("Open a project to export Spring endpoints.".into()))?;
            let task=host::begin_task("Exporting Spring endpoints as OpenAPI",true);
            let found=self.routes(&project,COMMAND_BUDGET);
            if found.scanning&&!found.walked{return Err(Error::Failed("Spring routes are still being indexed; try again in a moment.".into()))}
            let mut skipped=Vec::new();
            if let Some(note)=&found.note{skipped.push(format!("some endpoints ({note})"));}
            let mut controllers:Vec<String>=found.routes.iter().filter_map(|r|r.location.as_ref().map(|l|l.path.clone())).collect();
            controllers.sort();controllers.dedup();
            let sources=dto_schemas::Sources::collect(&project,&controllers,&mut skipped,&||task.is_cancelled())?;
            let document=openapi_export::export_with_registry(&found.routes, |route| route.location.as_ref().and_then(|location|sources.text(&location.path)),Some(&sources));
            if document.len()>1<<20{return Err(Error::Failed("The OpenAPI document is too large to copy (over 1 MiB).".into()))}
            host::copy_to_clipboard(&document)
                .map_err(|e| ui_kit::denied::map(e, "Copy OpenAPI export", "clipboard", PLUGIN_NAME))?;
            if skipped.is_empty(){host::notify(NotificationLevel::Info,"Spring Endpoints","Copied the OpenAPI document to the clipboard.");}else{host::notify(NotificationLevel::Info,"Spring Endpoints",&format!("Copied the OpenAPI document to the clipboard. It leaves out: {}.",skipped.join(", ")));}
            return Ok(None);
        }
        if id != "copy-endpoint-curl" && id != "create-endpoint-http" {
            return Err(Error::NotFound(format!("command {id}")));
        }
        let route = match arg {
            Some(key) => self.route_list().route(key).cloned(),
            None => self.pick_route(ctx)?,
        };
        let Some(mut route) = route else {
            return Ok(None);
        };
        if !select_http_method(&mut route)? { return Ok(None); }
        if id == "create-endpoint-http" {
            request_examples::create(&route,ctx)?;
            return Ok(None);
        }
        routes::copy_as_curl(&route, DEFAULT_BASE_URL, PLUGIN_NAME)?;
        Ok(None)
    }
}

const PLUGIN_NAME: &str = "Spring Endpoints Tools";

pub(crate) fn copy_after_write_denied(content: &str) -> Result<(), Error> {
    host::notify(
        NotificationLevel::Info,
        "Spring Endpoints",
        &ui_kit::denied::message("Create HTTP Request File", "project.write", PLUGIN_NAME),
    );

    host::copy_to_clipboard(content)
        .map_err(|e| ui_kit::denied::map(e, "Copy to clipboard", "clipboard", PLUGIN_NAME))
}

pub const WINDOW_SECTION: &str = "web";

pub const OWNER_WINDOW: &str = "endpoints";

pub(crate) fn refresh(_view: &str) {
    host::refresh_view(OWNER_WINDOW);
}

thread_local! {
    static PROBLEMS: std::cell::RefCell<Vec<Diagnostic>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The diagnostics belong to the owner plugin, which publishes every module's together: a publish replaces the whole file.
#[cfg(test)]
fn publish(_path: &str, problems: &[Diagnostic]) {
    PROBLEMS.set(problems.to_vec());
}

pub fn take_problems() -> Vec<Diagnostic> {
    PROBLEMS.take()
}

const DEPENDENCY_MARKERS: [&str; 1] = ["spring"];

/// True when unsure (see `spring_text::build::applies`).
pub fn applies(project: &openidea_plugin::Project) -> bool {
    spring_text::build::applies(project, &DEPENDENCY_MARKERS)
}

openidea_plugin::register_plugin!(window::Endpoints, providers, views, endpoints, diagnostics, hover, file_actions, language_features, injections);

#[cfg(test)]
impl WebTools {
    fn legacy_parser_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        if is_xml(doc) {
            let found = xml_checks::check(&doc.text);
            let mut diagnostics: Vec<_> = found.iter().map(|f| f.diagnostic(&doc.text)).collect();
            diagnostics.extend(self.unresolved_views(doc));
            publish(&doc.path, &diagnostics);
            return Vec::new();
        }
        let mut diagnostics: Vec<Diagnostic> = problems(doc).into_iter().map(|p| p.diagnostic).collect();
        diagnostics.extend(self.checked(doc));
        diagnostics.extend(self.unresolved_views(doc));
        diagnostics.extend(self.api_version_hints(doc));
        publish(&doc.path, &diagnostics);
        self.editor_markers(doc)
    }
}

#[cfg(test)]
mod tests {
    fn warmed() -> WebTools {
        let mut tools = WebTools::default();
        if let Some(project) = host::projects().unwrap_or_default().first() {
            tools.live_warm(project, COMMAND_BUDGET, true);
        }
        tools
    }
    #[test]
    fn marker_icons_are_declared_in_the_manifest() {
        let manifest = include_str!("../plugin.toml");
        for icon in [crate::ROUTE_MARKER, crate::TEMPLATE_MARKER] {
            assert!(manifest.contains(&format!("icon = \"{icon}\"")));
            assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(icon).is_file());
        }
    }

    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::views::ListItem;

    const JAVA_ROUTES: &str = r#"package com.acme;

@Configuration
class Routes {
    @Bean
    RouterFunction<ServerResponse> routes() {
        return RouterFunctions.route().GET("/x", this::list)
            .path("/api", b -> b.GET("/users", Handler::list))
            .build();
    }

    ServerResponse list(ServerRequest r) { return null; }
}
"#;

    const API: &str = r#"package com.acme.web;

@RestController
@RequestMapping("/owners/{ownerId}")
class PetApi {
    @GetMapping("/pets/{petId}")
    Pet get(@PathVariable int ownerId, @PathVariable("id") int petId) { return null; }

    @GetMapping("/visits/{visitId}")
    List<Visit> visits(@PathVariable Map<String, String> vars) { return null; }

    @PostMapping("/pets/{petId}/edit")
    String edit(@Valid Pet pet, BindingResult result) { return null; }

    @DeleteMapping({"/pets/{petId}", "/pets"})
    void delete(@PathVariable(name = "petId", required = false) Integer petId, @PathVariable String ownerId) {}

    @GetMapping(PATH)
    void constant(@PathVariable String nope) {}

    @GetMapping("/files/{*path}/{rev:[0-9]{2}}")
    void file(@PathVariable String path, @PathVariable String rev, @PathVariable String ownerId) {}
}
"#;

    fn doc(path: &str, text: &str) -> Document {
        Document { path: format!("/project/{path}"), language: "java".into(), version: 1, text: text.into() }
    }

    #[test]
    fn reads_uri_template_variables() {
        let names = |p: &str| variables(p).into_iter().map(|v| v.0).collect::<Vec<_>>();
        assert_eq!(names("/a/{id}/{name:[a-z]{2,}}/{*rest}"), ["id", "name", "rest"]);
        assert_eq!(names("/a/${base}/{x}"), ["x"]);
        assert_eq!(variables("/a/{id}")[0], ("id".into(), 3, 7));
    }

    #[test]
    fn checks_path_variables_and_fixes_the_name() {
        let _host = MockHost::install();
        let mut tools = warmed();
        let d = doc("src/main/java/com/acme/web/PetApi.java", API);
        tools.legacy_parser_markers(&d);
        let problems = &take_problems();
        let found: Vec<_> = problems.iter().map(|p| (p.code.as_deref().unwrap(), p.range.start.line, p.message.as_str())).collect();
        assert_eq!(
            found,
            [
                ("unknown-path-variable", 6, "`id` is not a variable of the mapped path `/owners/{ownerId}/pets/{petId}`."),
                ("unbound-path-variable", 5, "No @PathVariable parameter takes the `petId` variable."),
            ]
        );
        assert_eq!(problems[1].range, Range::new(Position::new(5, 23), Position::new(5, 30)));

        let actions = tools.code_actions(&d, Range::new(Position::new(6, 40), Position::new(6, 40)));
        assert_eq!(actions.len(), 1);
        let edit = &actions[0].edits[0].1[0];
        assert_eq!(edit.text, "PathVariable(\"petId\")");
        let fixed = API.replace("@PathVariable(\"id\")", "@PathVariable(\"petId\")");
        assert_eq!(path_problems(&fixed, &java::parse(&fixed)).len(), 0);
    }

    #[test]
    fn generate_actions_follow_the_quick_fixes() {
        let mut tools = WebTools::default();
        let source = "package a;\nimport org.springframework.web.bind.annotation.*;\n@RestController\nclass A {\n    @GetMapping(\"/{id}\")\n    String x(@PathVariable(\"idd\") long id) { return \"\"; }\n\n}\n";
        let d = doc("A.java", source);
        let at = Position::new(6, 0);

        let kinds: Vec<_> = tools.code_actions(&d, Range::new(at, at)).into_iter().map(|a| a.kind).collect();
        assert_eq!(kinds, vec![Some("refactor".to_string()); 2]);

        let at = Position::new(5, 30);
        let kinds: Vec<_> = tools.code_actions(&d, Range::new(at, at)).into_iter().map(|a| a.kind.unwrap()).collect();
        assert_eq!(kinds, vec!["quickfix"]);
    }

    const CONTROLLER: &str = r#"package com.acme.web;

@Controller
@RequestMapping("/users")
class UserController {
    @GetMapping
    String list(Model model) {
        if (model == null) return "redirect:/";
        return "users/list";
    }

    @GetMapping("/{id}")
    ModelAndView show(@PathVariable long id) {
        ModelAndView mav = new ModelAndView("users/show");
        mav.setViewName("fragments/card :: card");
        return mav;
    }

    @GetMapping("/legacy")
    String legacy() { return "home"; }

    @ResponseBody @GetMapping("/raw")
    String raw() { return "users/list"; }

    String helper() { return "users/list"; }
}
"#;

    fn web_project() -> MockHost {
        let host = MockHost::install();
        host.add_file("src/main/resources/templates/users/list.html", "<html/>");
        host.add_file("src/main/resources/templates/users/show.ftlh", "");
        host.add_file("src/main/resources/templates/fragments/card.html", "");
        host.add_file("src/main/resources/static/users/list.html", "");
        host.add_file("src/main/webapp/WEB-INF/pages/home.jsp", "");
        host.add_file("src/main/resources/application.properties", "spring.mvc.view.prefix=/WEB-INF/pages/\nspring.mvc.view.suffix=.jsp\n");
        host
    }

    #[test]
    fn finds_view_names_of_controller_methods() {
        let file = java::parse(CONTROLLER);
        let names: Vec<_> = view_literals(CONTROLLER, &file).into_iter().map(|s| s.value).collect();
        assert_eq!(names, ["users/list", "users/show", "fragments/card :: card", "home"]);
    }

    #[test]
    fn every_template_of_a_view_name_gets_a_marker_on_its_line() {
        let host = web_project();
        host.add_file("src/main/resources/templates/users/list.ftlh", "");
        let mut tools = warmed();
        let d = doc("src/main/java/com/acme/web/UserController.java", CONTROLLER);
        let markers = tools.legacy_parser_markers(&d);
        let list: Vec<_> = markers.iter().filter(|m| m.line == 8).collect();
        let tooltips: Vec<_> = list.iter().map(|m| m.tooltip.as_str()).collect();
        assert_eq!(tooltips, ["View template resources/templates/users/list.html", "View template resources/templates/users/list.ftlh"]);
        assert!(list.iter().all(|m| m.icon.as_deref() == Some(TEMPLATE_MARKER) && m.target.is_some()));
        assert_eq!(markers.iter().filter(|m| m.line == 13).count(), 1, "one template, one marker");
    }

    #[test]
    fn links_view_names_to_templates() {
        let _host = web_project();
        let mut tools = warmed();
        let d = doc("src/main/java/com/acme/web/UserController.java", CONTROLLER);
        let markers = tools.legacy_parser_markers(&d);
        let found: Vec<_> = markers.iter().filter(|m| !m.tooltip.starts_with("URL:")).map(|m| (m.line, m.tooltip.as_str())).collect();
        assert_eq!(
            found,
            [
                (8, "View template resources/templates/users/list.html"),
                (13, "View template resources/templates/users/show.ftlh"),
                (14, "View template resources/templates/fragments/card.html"),
                (19, "View template webapp/WEB-INF/pages/home.jsp"),
            ]
        );
        let targets = tools.definition(&d, Position::new(8, 18));
        assert_eq!(targets, [Location { path: "/project/src/main/resources/templates/users/list.html".into(), range: Range::default() }]);
        assert!(tools.definition(&d, Position::new(22, 28)).is_empty(), "@ResponseBody returns a body");
    }

    #[test]
    fn router_handler_reference_goes_to_the_method() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Routes.java", JAVA_ROUTES);
        host.add_file(
            "src/main/java/com/acme/Handler.java",
            "class Handler {\n    ServerResponse list(ServerRequest r) { return null; }\n}\n",
        );
        let mut tools = warmed();
        let d = doc("src/main/java/com/acme/Routes.java", JAVA_ROUTES);
        let markers = tools.legacy_parser_markers(&d);
        let found: Vec<_> = markers.iter().map(|m| (m.line, m.target.as_ref().map(|t| t.path.rsplit('/').next().unwrap()))).collect();
        assert_eq!(found[0], (6, Some("Routes.java")));
        assert_eq!(found[1], (7, Some("Handler.java")));
        assert!(markers.iter().all(|m| m.tooltip.starts_with("Handler of ")));
        let at = Position::new(7, 50);
        let targets = tools.definition(&d, at);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].range.start.line, 1);
        assert!(tools.definition(&d, Position::new(6, 52)).len() == 1, "this::list, on the reference");
        assert!(tools.definition(&d, Position::new(6, 15)).is_empty());
    }

    const USERS: &str = r#"@RestController
@RequestMapping("/users")
class UserApi {
    @GetMapping
    List<User> all() { return null; }

    @GetMapping("/{id}")
    User get(@PathVariable long id) { return null; }

    @DeleteMapping("/{id}")
    void delete(@PathVariable long id) {}

    @GetMapping("/{id}/orders") List<Order> orders(@PathVariable long id) { return null; }
    @GetMapping("/by-{name}") User byName(@PathVariable String name) { return null; }
    @PutMapping User replace() { return null; }
}
"#;

    const USER_TESTS: &str = r#"import org.springframework.test.web.servlet.MockMvc;
import org.springframework.test.web.servlet.request.MockMvcRequestBuilders;
import org.springframework.test.web.reactive.server.WebTestClient;
import org.springframework.boot.test.web.client.TestRestTemplate;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.*;
@WebMvcTest
class UserApiTests {
    @Autowired MockMvc mockMvc;
    @Autowired WebTestClient webTestClient;
    @Autowired TestRestTemplate restTemplate;

    @Test void get() throws Exception {
        mockMvc.perform(get("/users/{id}", 1)).andExpect(status().isOk());
        mockMvc.perform(MockMvcRequestBuilders.delete("/users/7"));
        mockMvc.perform(post("/users"));
        webTestClient.get().uri("/users").exchange();
        webTestClient.get().uri("/users/by-bob?x=1").exchange();
        restTemplate.getForObject("/users/1/orders", String.class);
        restTemplate.exchange("/users/3", HttpMethod.DELETE, null, Void.class);
        Map<String, String> m = new HashMap<>(); m.put("/users", "x");
    }
}
"#;

    #[test]
    fn mockmvc_perform_get_links_to_handler() {
        let host = MockHost::install();
        host.set_setting("springWeb.showUrlMarkers", "true");
        host.add_file("src/main/java/com/acme/UserApi.java", USERS);
        let mut tools = warmed();
        let d = doc("src/test/java/com/acme/UserApiTests.java", USER_TESTS);
        let markers = tools.legacy_parser_markers(&d);
        let found: Vec<_> = markers.iter().map(|m| (m.line, m.target.as_ref().unwrap().range.start.line)).collect();
        assert_eq!(found, [(12, 7), (13, 10), (15, 4), (16, 7), (16, 13), (17, 12), (18, 10)]);
        assert_eq!(markers[0].tooltip, "Handler of GET /users/{id}");
        assert_eq!(markers[3].icon, markers[4].icon, "both handlers of /users/by-bob share one gutter button");

        let at = Position::new(12, 30);
        let targets = tools.definition(&d, at);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, "/project/src/main/java/com/acme/UserApi.java");
        assert_eq!(targets[0].range.start.line, 7);
        assert!(tools.definition(&d, Position::new(14, 30)).is_empty(), "no POST /users mapping");
        let main = doc("src/main/java/com/acme/UserApi.java", USERS);
        let urls: Vec<_> = tools.legacy_parser_markers(&main).into_iter().map(|m| (m.line, m.tooltip, m.target.is_none())).collect();
        assert!(urls.contains(&(7, "URL: GET /users/{id}".to_owned(), true)), "{urls:?}");
        assert!(urls.iter().any(|u| u.1 == "URL: PUT /users"), "{urls:?}");
    }

    #[test]
    fn offers_mapping_snippets_in_controller_bodies() {
        let _host = MockHost::install();
        let src = "@RestController\nclass A {\n    getm\n    void x() { getm }\n}\nclass B {\n    getm\n}\n";
        let mut tools = warmed();
        let d = doc("src/main/java/A.java", src);
        let items = tools.completion(&d, Position::new(2, 8), None);
        assert_eq!(items.len(), 1);
        assert!(items[0].snippet && items[0].insert_text.starts_with("@GetMapping(\"${1:/}\")\npublic "));
        assert!(tools.completion(&d, Position::new(3, 17), None).is_empty(), "inside a method body");
        assert!(tools.completion(&d, Position::new(6, 8), None).is_empty(), "not a controller");
        let reqm = src.replace("getm\n    void", "m\n    void");
        let all = tools.completion(&doc("src/main/java/A.java", &reqm), Position::new(2, 5), None);
        assert!(all.is_empty(), "no abbreviation starts with m");
        let kotlin = Document {
            path: "/project/A.kt".into(),
            language: "kotlin".into(),
            version: 1,
            text: "@Controller\nclass A {\n    pos\n}\n".into(),
        };
        let items = tools.completion(&kotlin, Position::new(2, 7), None);
        assert!(items[0].insert_text.contains("fun ${3:post}"));
    }

    #[test]
    fn offers_controller_and_router_snippets_by_scope() {
        let _host = MockHost::install();
        let mut tools = warmed();
        let labels = |tools: &mut WebTools, path: &str, text: &str, line: u32, column: u32| -> Vec<String> {
            let d = Document { path: format!("/project/{path}"), language: "java".into(), version: 1, text: text.into() };
            tools.completion(&d, Position::new(line, column), None).into_iter().map(|i| i.label).collect()
        };
        assert_eq!(labels(&mut tools, "A.java", "restc\n", 0, 5), ["restc"]);
        assert_eq!(labels(&mut tools, "A.java", "class A {\n}\nrestc\n", 2, 5), ["restc"]);
        assert!(labels(&mut tools, "A.java", "class A {\n    restc\n}\n", 1, 9).is_empty());
        assert_eq!(labels(&mut tools, "A.java", "class A {\n    rout\n}\n", 1, 8), ["router"]);
        assert_eq!(labels(&mut tools, "A.java", "@Controller\nclass A {\n    exh\n}\n", 2, 7), ["exh"]);
        assert!(labels(&mut tools, "A.java", "class A {\n    exh\n}\n", 1, 7).is_empty());
        let kotlin =
            Document { path: "/project/A.kt".into(), language: "kotlin".into(), version: 1, text: "class A {\n    router\n}\n".into() };
        let items = tools.completion(&kotlin, Position::new(1, 10), None);
        assert!(items[0].insert_text.contains("= router {"));
    }

    #[test]
    fn url_markers_can_be_turned_off() {
        let host = MockHost::install();
        host.set_setting("springWeb.showUrlMarkers", "false");
        let mut tools = warmed();
        let main = doc("src/main/java/com/acme/UserApi.java", USERS);
        assert!(tools.legacy_parser_markers(&main).is_empty());
    }

    #[test]
    fn completes_mapping_urls_in_test_literals() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/UserApi.java", USERS);
        let mut tools = warmed();
        let typing = USER_TESTS.replace("get(\"/users/{id}\", 1)", "get(\"\", 1)");
        let d = doc("src/test/java/com/acme/UserApiTests.java", &typing);
        let items = tools.completion(&d, Position::new(12, 29), None);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["/users", "/users/by-{name}", "/users/{id}", "/users/{id}/orders"]);
        let get = USER_TESTS.replace("get(\"/users/{id}\", 1)", "get(\"/users/\", 1)");
        let d = doc("src/test/java/com/acme/UserApiTests.java", &get);
        let labels: Vec<_> = tools.completion(&d, Position::new(12, 31), None).into_iter().map(|i| i.label).collect();
        assert!(labels.contains(&"/users/{id}".to_owned()) && labels.contains(&"/users".to_owned()), "{labels:?}");
        assert!(tools.completion(&d, Position::new(2, 5), None).is_empty());
    }

    #[test]
    fn static_resource_literals_go_to_the_file() {
        let host = MockHost::install();
        host.add_file("src/main/resources/static/css/app.css", "");
        host.add_file("src/main/resources/public/js/a.js", "");
        let mut tools = warmed();
        let src = "class C { String a = \"/css/app.css\"; String b = \"classpath:/public/js/a.js\"; String c = \"/nope.css\"; String d = \"/users/list\"; }";
        let d = doc("src/main/java/C.java", src);
        let at = |needle: &str| Position::new(0, src.find(needle).unwrap() as u32 + 2);
        let mut paths = |p| tools.definition(&d, p).into_iter().map(|l| l.path).collect::<Vec<_>>();
        assert_eq!(paths(at("/css")), ["/project/src/main/resources/static/css/app.css"]);
        assert_eq!(paths(at("classpath")), ["/project/src/main/resources/public/js/a.js"]);
        assert!(paths(at("/nope")).is_empty());
        assert!(paths(at("/users")).is_empty());
    }

    #[test]
    fn template_resource_links_resolve_static_assets_and_skip_dynamic_or_commented_values() {
        let host = MockHost::install();
        host.add_file("src/main/resources/static/css/app.css", "");
        let text = "<!-- <link th:href=\"@{/css/app.css}\"> -->\n<link th:href=\"@{/css/app.css?v=1}\"><script th:src=\"@{${asset}}\"></script>";
        let d = Document { path: "/project/src/main/resources/templates/home.html".into(), language: "html".into(), text: text.into(), version:1 };
        let mut tools = warmed();
        assert!(tools.definition(&d, position(text, text.find("/css/app.css").unwrap()+2)).is_empty());
        let found = tools.definition(&d, position(text, text.rfind("/css/app.css").unwrap()+2));
        assert_eq!(found[0].path,"/project/src/main/resources/static/css/app.css");
        assert!(tools.definition(&d, position(text,text.find("${asset}").unwrap()+2)).is_empty());
        assert!(static_resource("/../app.css").is_none());
    }

    #[test]
    fn thymeleaf_fragment_definition_uses_exact_fragment_declaration() {
        let host = MockHost::install();
        let fragment = "<header th:fragment=\"title(user)\">Hi</header>";
        host.add_file("src/main/resources/templates/parts/header.html",fragment);
        let source = "<div th:replace=\"~{parts/header :: title}\"></div>";
        let d = Document { path:"/project/src/main/resources/templates/home.html".into(), language:"html".into(), text:source.into(), version:1 };
        let mut tools = warmed();
        let found = tools.definition(&d,position(source,source.find("title").unwrap()+2));
        assert_eq!(found.len(),1);
        assert_eq!(found[0].path,"/project/src/main/resources/templates/parts/header.html");
        assert_eq!(&fragment[offset(fragment,found[0].range.start).unwrap()..offset(fragment,found[0].range.end).unwrap()],"title");
    }

    #[test]
    fn thymeleaf_fragment_completion_replaces_only_selector_and_supports_unsaved_local_fragments() {
        let host=MockHost::install();
        host.add_file("src/main/resources/templates/parts.html","<div th:fragment='header(user)'/><footer th:fragment='footer'/>");
        let source="<div th:replace='~{parts :: hea}'/>";
        let d=Document{path:"/project/src/main/resources/templates/home.html".into(),language:"html".into(),text:source.into(),version:1};
        let mut tools=warmed();
        let items=tools.completion(&d,position(source,source.find("hea").unwrap()+3),None);
        let item=items.iter().find(|i|i.label=="header").unwrap();
        let r=item.replace.unwrap();assert_eq!(&source[offset(source,r.start).unwrap()..offset(source,r.end).unwrap()],"hea");
        let local="<div th:fragment='unsaved'/><span th:insert='~{:: uns}'/>";
        let d=Document{text:local.into(),..d};
        assert!(tools.completion(&d,position(local,local.rfind("uns").unwrap()+3),None).iter().any(|i|i.label=="unsaved"));
    }

    #[test]
    fn mvc_model_names_complete_and_navigate_only_in_their_returned_template(){
        let host=MockHost::install();
        let source="@Controller class C { @GetMapping(\"/show\") String show(Model model){model.addAttribute(\"account\",user); return \"users/show\";} }";
        host.add_file("src/main/java/C.java",source);
        let text="<p th:text=\"${account.name}\"/>";
        let d=Document{path:"/project/src/main/resources/templates/users/show.html".into(),language:"html".into(),text:text.into(),version:1};
        let mut tools=warmed();let cursor=position(text,text.find("account").unwrap()+2);
        let targets=tools.definition(&d,cursor);assert_eq!(targets.len(),1);
        assert_eq!(&source[offset(source,targets[0].range.start).unwrap()..offset(source,targets[0].range.end).unwrap()],"account");
        assert!(tools.completion(&d,cursor,None).iter().any(|i|i.label=="account"));
        let unfinished="<p th:text=\"${acc\"/>";
        let editing=Document{text:unfinished.into(),..d.clone()};
        assert!(tools.completion(&editing,position(unfinished,unfinished.find("acc").unwrap()+3),None).iter().any(|i|i.label=="account"));
        let other=Document{path:"/project/src/main/resources/templates/users/list.html".into(),..d.clone()};assert!(tools.definition(&other,cursor).is_empty());
        host.add_file("src/main/java/C.java",&source.replace("users/show","users/list"));
        assert_eq!(tools.definition(&d,cursor).len(),1,"providers read the cache, not the disk");
        tools.models.on_events(&[openidea_plugin::Event::FileSaved("/project/src/main/java/C.java".into())],Instant::now()+COMMAND_BUDGET);
        assert!(tools.definition(&d,cursor).is_empty(),"the saved file moved the model name away");
    }

    #[test]
    fn unresolved_views_are_a_hint_without_a_custom_resolver() {
        let host = web_project();
        let mut tools = warmed();
        let src = CONTROLLER.replace("return \"home\";", "return \"nowhere\";");
        let d = doc("src/main/java/com/acme/web/UserController.java", &src);
        tools.legacy_parser_markers(&d);
        let hints: Vec<_> = take_problems().into_iter().filter(|p| p.code.as_deref() == Some("unresolved-view")).collect();
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].severity, Severity::Hint);
        assert!(hints[0].message.contains("nowhere"));

        host.set_setting("springWeb.reportUnresolvedViews", "false");
        tools.legacy_parser_markers(&d);
        assert!(take_problems().iter().all(|p| p.code.as_deref() != Some("unresolved-view")));
    }

    const MVC_NS: &str = "http://www.springframework.org/schema/mvc";
    fn view_controller_xml(view: &str) -> String {
        format!("<beans xmlns=\"http://www.springframework.org/schema/beans\" xmlns:mvc=\"{MVC_NS}\"><mvc:view-controller path=\"/x\" view-name=\"{view}\"/></beans>")
    }

    #[test]
    fn xml_view_controller_navigates_and_hints_only_for_default_resolution() {
        let host = web_project();
        let mut tools = warmed();
        let text = view_controller_xml("users/list");
        let d = doc("src/main/resources/mvc.xml", &text);
        let found = tools.definition(&d, position(&text, text.find("users/list").unwrap() + 2));
        assert_eq!(found[0].path, "/project/src/main/resources/templates/users/list.html");

        let text = view_controller_xml("nowhere");
        let d = doc("src/main/resources/mvc.xml", &text);
        tools.legacy_parser_markers(&d);
        assert!(take_problems().iter().any(|p| p.code.as_deref() == Some("unresolved-view")));
        let dynamic = view_controller_xml("${v}");
        assert!(tools.definition(&doc("src/main/resources/mvc.xml", &dynamic), position(&dynamic, dynamic.find("${").unwrap() + 3)).is_empty());

        host.add_file("src/main/resources/other.xml", "<beans><bean class=\"x.InternalResourceViewResolver\"/></beans>");
        tools.legacy_parser_markers(&d);
        assert!(take_problems().iter().all(|p| p.code.as_deref() != Some("unresolved-view")));
    }

    #[test]
    fn java_config_add_view_controller_resolves_like_controller_views() {
        let host = web_project();
        let mut tools = warmed();
        let imports = "import org.springframework.web.servlet.config.annotation.WebMvcConfigurer;\nimport org.springframework.web.servlet.config.annotation.ViewControllerRegistry;\n";
        let src = format!("{imports}class Cfg implements WebMvcConfigurer {{ public void addViewControllers(ViewControllerRegistry registry) {{ registry.addViewController(\"/x\").setViewName(\"users/list\"); registry.addViewController(\"/y\").setViewName(\"nowhere\"); }} }}");
        let d = doc("src/main/java/com/acme/Cfg.java", &src);
        let found = tools.definition(&d, position(&src, src.find("users/list").unwrap() + 2));
        assert_eq!(found[0].path, "/project/src/main/resources/templates/users/list.html");
        tools.legacy_parser_markers(&d);
        let hints: Vec<_> = take_problems().into_iter().filter(|p| p.code.as_deref() == Some("unresolved-view")).collect();
        assert_eq!(hints.len(), 1);
        assert!(hints[0].message.contains("nowhere"));
        let other = src.replace("import org.springframework.web.servlet.config.annotation.WebMvcConfigurer;", "import com.other.WebMvcConfigurer;");
        assert!(tools.definition(&doc("src/main/java/com/acme/Cfg.java", &other), position(&other, other.find("users/list").unwrap() + 2)).is_empty());
        drop(host);
    }

    #[test]
    fn thymeleaf_id_selector_navigates_only_to_a_unique_element() {
        let host = web_project();
        host.add_file("src/main/resources/templates/layout.html", "<html><div id=\"main\"/><p id=\"dup\"/><p id=\"dup\"/></html>");
        let mut tools = warmed();
        let text = "<div th:replace=\"~{layout :: #main}\"/><div th:replace=\"~{layout :: #dup}\"/><div th:replace=\"~{layout :: .box}\"/>";
        let d = Document { language: "html".into(), ..doc("src/main/resources/templates/home.html", text) };
        let main = tools.definition(&d, position(text, text.find("#main").unwrap() + 1));
        assert_eq!(main.len(), 1);
        assert_eq!(main[0].path, "/project/src/main/resources/templates/layout.html");
        assert_eq!(main[0].range.start.character, 15);
        assert!(tools.definition(&d, position(text, text.find("#dup").unwrap() + 1)).is_empty());
        assert!(tools.definition(&d, position(text, text.find(".box").unwrap() + 1)).is_empty());
    }

    #[test]
    fn unresolved_views_are_quiet_with_a_custom_view_resolver() {
        let host = web_project();
        host.add_file("src/main/java/com/acme/Cfg.java", "class Cfg { ViewResolver r() { return null; } }");
        let mut tools = warmed();
        let src = CONTROLLER.replace("return \"home\";", "return \"nowhere\";");
        let d = doc("src/main/java/com/acme/web/UserController.java", &src);
        tools.legacy_parser_markers(&d);
        assert!(take_problems().iter().all(|p| p.code.as_deref() != Some("unresolved-view")));
    }

    #[test]
    fn kotlin_controllers_get_path_variable_checks() {
        let _host = MockHost::install();
        let src =
            "@RestController\nclass A {\n    @GetMapping(\"/u/{id}\")\n    fun get(@PathVariable(\"uid\") id: Long): String = \"\"\n}\n";
        let d = Document { path: "/project/src/main/kotlin/A.kt".into(), language: "kotlin".into(), version: 1, text: src.into() };
        warmed().legacy_parser_markers(&d);
        let codes: Vec<_> = take_problems().iter().map(|p| p.code.clone().unwrap()).collect();
        assert!(codes.contains(&"unknown-path-variable".to_owned()), "{codes:?}");
    }

    #[test]
    fn mvc_xml_is_checked() {
        let _host = MockHost::install();
        let text = r#"<beans xmlns:mvc="http://www.springframework.org/schema/mvc"><mvc:resources mapping="/x/**"/></beans>"#;
        let d = Document { path: "/project/mvc.xml".into(), language: "xml".into(), version: 1, text: text.into() };
        assert!(warmed().legacy_parser_markers(&d).is_empty());
        assert_eq!(take_problems()[0].code.as_deref(), Some("missing-attribute"));
    }

    #[test]
    fn api_version_in_tests_needs_an_inserter() {
        let host = MockHost::install();
        host.add_file("src/main/resources/application.properties", "server.port=1\n");
        let test = "class T { void t() { client.get().uri(\"/x\").apiVersion(\"1.1\").exchange(); } }";
        let d = doc("src/test/java/T.java", test);
        let mut tools = warmed();
        let hints = |tools: &mut WebTools| {
            tools.legacy_parser_markers(&d);
            take_problems().iter().filter(|p| p.code.as_deref() == Some("api-version-without-inserter")).count()
        };
        assert_eq!(hints(&mut tools), 1);

        host.add_file("src/main/java/Cfg.java", "class Cfg { ApiVersionInserter i; }");
        let mut tools = warmed();
        assert_eq!(hints(&mut tools), 0);
    }

    #[test]
    fn completes_view_names() {
        let _host = web_project();
        let mut tools = warmed();
        let typing = CONTROLLER.replace("return \"home\";", "return \"us\";");
        let d = doc("src/main/java/com/acme/web/UserController.java", &typing);
        let items = tools.completion(&d, Position::new(19, 31), None);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["fragments/card", "users/list", "users/show", "home"]);
        assert_eq!(items[0].replace, Some(Range::new(Position::new(19, 30), Position::new(19, 32))));
        assert!(tools.completion(&d, Position::new(24, 30), None).is_empty(), "not a handler method");
    }

    #[test]
    fn fixed_file_publishes_empty_list() {
        let _host = MockHost::install();
        _host.add_file("src/main/resources/application.properties", "server.port=1\n");
        let path = "src/main/java/com/acme/web/Api.java";
        let broken = "package a;\nimport org.springframework.web.bind.annotation.*;\n@RestController\nclass Api {\n    @GetMapping(path = \"/é😀\", version = \"2\")\n    String a() { return \"\"; }\n    @InitBinder\n    String b(WebDataBinder d) { return \"\"; }\n}\n";
        let mut tools = warmed();
        tools.legacy_parser_markers(&doc(path, broken));
        let found = &take_problems();
        let codes: Vec<_> = found.iter().filter_map(|d| d.code.as_deref()).collect();
        assert_eq!(codes, ["api-version-mismatch", "init-binder-signature"]);
        assert_eq!(found[0].range.start.line, 4);
        assert_eq!(found[0].range.end.character - found[0].range.start.character, 3);

        let fixed = broken.replace(", version = \"2\"", "").replace("String b(", "void b(").replace("return \"\"; }\n}", "}\n}");
        tools.legacy_parser_markers(&doc(path, &fixed));
        assert!(take_problems().is_empty());
    }

    const ENDPOINT_VIEW: &str = "endpoints";

    const API_CONTROLLER: &str = r#"package com.acme;

@RestController
@RequestMapping("/api")
class Users {
    @PostMapping("/users")
    void create() {}

    @GetMapping("/users")
    void list() {}

    @RequestMapping("/any")
    void any() {}

    @GetMapping(PATH)
    void constant() {}

    @GetMapping("/odd:name/{id}")
    void odd() {}
}
"#;

    fn rows(view: View) -> Vec<ListItem> {
        match view.body {
            Body::List(items) => items,
            _ => panic!("not a list"),
        }
    }

    fn row_key(host: &MockHost, tools: &mut WebTools, path: &str, detail: &str) -> String {
        let rows = rows(tools.render(ENDPOINT_VIEW, None, &host.invocation()).unwrap());
        rows.iter().find(|i| i.label == path && i.detail.as_deref() == Some(detail)).unwrap().key.clone()
    }

    fn message(view: &View) -> &str {
        match &view.body {
            Body::Message(text) => text,
            _ => panic!("not a message"),
        }
    }

    #[test]
    fn endpoints_view_lists_routes_flat_with_controller_detail() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        host.add_file("src/main/java/com/acme/Routes.java", JAVA_ROUTES);
        let mut tools = warmed();
        let view = tools.render(ENDPOINT_VIEW, None, &host.invocation()).unwrap();
        assert_eq!(view.status.as_deref(), Some("6 endpoints"));
        let rows = rows(view);
        let labels: Vec<_> = rows.iter().map(|i| (i.label.as_str(), i.detail.as_deref())).collect();
        assert_eq!(
            labels,
            [
                ("/api/any", Some("[ANY] Users@any")),
                ("/api/odd:name/{id}", Some("[GET] Users@odd")),
                ("/api/users", Some("[GET] Handler@list")),
                ("/api/users", Some("[GET] Users@list")),
                ("/api/users", Some("[POST] Users@create")),
                ("/x", Some("[GET] Routes@list")),
            ]
        );
        assert_eq!(rows[4].icon.as_deref(), Some("web"));
    }

    #[test]
    fn annotated_route_identity_keeps_its_method_and_exact_source_location() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        let mut tools = warmed();
        let found = tools.routes(&host.invocation().project().unwrap(), COMMAND_BUDGET);
        assert!(found.note.is_none());
        let routes = found.routes;
        for route in &routes {
            assert_eq!(route.handler.class, "Users");
            let member = route.handler.member.as_deref().expect("annotated method identity");
            let location = route.location.as_ref().expect("source location");
            let start = offset(API_CONTROLLER, location.range.start).unwrap();
            let end = offset(API_CONTROLLER, location.range.end).unwrap();
            assert_eq!(&API_CONTROLLER[start..end], member);
        }
        assert_eq!(routes.iter().find(|r| r.method == "POST").unwrap().handler.member.as_deref(), Some("create"));
    }

    #[test]
    fn http_client_paths_complete_and_navigate_without_replacing_authority_or_query() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        host.add_file("pom.xml", "spring-web");
        let text = "GET https://localhost:8080/api/users?limit=10\n\nGET /body-is-not-a-request\n### next\nHEAD {{host}}/api/us\n";
        let d = Document { path: "/project/requests.http".into(), language: "http".into(), text: text.into(), version: 1 };
        let mut tools = warmed();
        let cursor = text.find("/api/users").unwrap() + 3;
        assert!(!tools.definition(&d, position(text, cursor)).is_empty());
        let items = tools.completion(&d, position(text, text.rfind("/api/us").unwrap() + 7), None);
        let users = items.iter().find(|i| i.label == "/api/users").expect("GET fallback for HEAD");
        let replacement = users.replace.unwrap();
        assert_eq!(&text[offset(text, replacement.start).unwrap()..offset(text, replacement.end).unwrap()], "/api/us");
        let found = endpoints::requests(&d.path, text);
        assert_eq!(found.len(), 2);
        assert_eq!(&text[found[0].at..found[0].end], "/api/users");
    }

    #[test]
    fn openapi_command_exports_fresh_complete_routes() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        let mut tools = warmed();
        tools.run_command("copy-endpoints-openapi", &host.invocation(), None).unwrap();
        assert_eq!(host.notifications().len(), 1);
        let data: serde_json::Value = serde_json::from_str(&host.clipboard().unwrap()).unwrap();
        assert_eq!(data["paths"]["/api/users"]["post"]["x-spring-handlers"][0], "Users@create");
        assert_eq!(data["x-openidea-unexported-mappings"].as_array().unwrap().len(),2);
    }

    #[test]
    fn http_completion_leaves_native_methods_owned_and_supplements_only_missing_head_paths(){
        let host=MockHost::install();host.add_file("src/C.java","@RestController class C { @GetMapping(\"/users/{id}\") void user(){} @RequestMapping(value=\"/native\",method=RequestMethod.HEAD) void head(){} @GetMapping(\"/native\") void get(){} }");
        let mut tools=warmed();
        let mut d=Document{path:"/project/requests.http".into(),language:"http".into(),version:1,text:"GET /us".into()};assert!(tools.completion(&d,Position::new(0,7),None).is_empty());
        d.text="HEAD /us".into();let items=tools.completion(&d,Position::new(0,8),None);assert!(items.iter().any(|i|i.label=="/users/{{id}}"));assert!(!items.iter().any(|i|i.label=="/native"));
    }

    #[test]
    fn controller_mappings_are_left_to_the_ide_and_functional_routes_are_published(){
        use openidea_plugin::endpoints::EndpointsProvider;
        let host=MockHost::install();host.add_file("pom.xml","spring-web");
        host.add_file("src/C.java","@RestController class C { @GetMapping(\"/annotated\") void route(){} }");
        host.add_file("src/R.java","class R { RouterFunction<ServerResponse> r() { return RouterFunctions.route(RequestPredicates.GET(\"/functional\"), this::list); } }");
        let mut owner=window::Endpoints::default();
        let published:Vec<_>=owner.endpoints(&host.invocation()).unwrap().into_iter().map(|e|e.path).collect();
        assert_eq!(published,["/functional"]);
    }

    #[test]
    fn saved_source_events_refresh_exported_routes_before_cache_ttl_and_template_creations_refresh_completion(){
        use openidea_plugin::endpoints::EndpointsProvider;
        let host=MockHost::install();host.add_file("pom.xml","spring-web");
        let routes=|path:&str|format!("class C {{ RouterFunction<ServerResponse> r() {{ return RouterFunctions.route(RequestPredicates.GET(\"{path}\"), this::list); }} }}");
        host.add_file("src/C.java",&routes("/old"));
        let mut owner=window::Endpoints::default();
        assert!(owner.endpoints(&host.invocation()).unwrap().iter().any(|e|e.path=="/old"));
        host.add_file("src/C.java",&routes("/new"));
        owner.handle_events(&[openidea_plugin::Event::FileSaved("/project/src/C.java".into())],&host.invocation());
        let endpoints=owner.endpoints(&host.invocation()).unwrap();assert!(endpoints.iter().any(|e|e.path=="/new"));assert!(!endpoints.iter().any(|e|e.path=="/old"));
        let source="@Controller class Page { @GetMapping(\"/\") String page(){return \"home\";} }";
        let d=doc("src/Page.java",source);let cursor=position(source,source.find("home").unwrap()+2);assert!(owner.completion(&d,cursor,None).is_empty());
        host.add_file("src/main/resources/templates/new.html","new");
        owner.handle_events(&[openidea_plugin::Event::FileSaved("/project/src/main/resources/templates/new.html".into())],&host.invocation());
        assert!(owner.completion(&d,cursor,None).iter().any(|template|template.label=="new"));
    }

    #[test]
    fn kotlin_literal_view_returns_complete_and_navigate_without_interpolation_guessing(){
        let host=MockHost::install();host.add_file("src/main/resources/templates/home.html","");host.add_file("src/main/resources/templates/users/show.html","");
        let source="@Controller\nclass Page {\n@GetMapping(\"/one\")\nfun one():String { return \"users/show\" }\n@GetMapping(\"/two\")\nfun two() = \"home\"\n@GetMapping(\"/dynamic\")\nfun dynamic() = \"$name\"\n}";
        let d=Document{path:"/project/src/Page.kt".into(),language:"kotlin".into(),version:1,text:source.into()};let mut tools=warmed();
        let found=tools.definition(&d,position(source,source.find("users/show").unwrap()+2));assert_eq!(found[0].path,"/project/src/main/resources/templates/users/show.html");
        let cursor=position(source,source.find("home").unwrap()+2);assert!(tools.definition(&d,cursor).iter().any(|location|location.path.ends_with("home.html")));assert!(tools.completion(&d,cursor,None).iter().any(|item|item.label=="users/show"));
        assert!(tools.definition(&d,position(source,source.find("$name").unwrap()+2)).is_empty());
    }

    #[test]
    fn controller_filter_narrows_the_endpoints() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        host.add_file("src/main/java/com/acme/Routes.java", JAVA_ROUTES);
        let mut tools = warmed();
        tools.render(ENDPOINT_VIEW, None, &host.invocation()).unwrap();
        host.answer_next_prompt(Some(&["Routes"]));
        tools.handle(ENDPOINT_VIEW, ViewEvent::Button("routes:filter".into()), &host.invocation()).unwrap();
        assert_eq!(host.refreshed_views(), [ENDPOINT_VIEW]);
        let view = tools.render(ENDPOINT_VIEW, None, &host.invocation()).unwrap();
        assert_eq!(view.status.as_deref(), Some("1 endpoints \u{b7} controller: Routes"));
        assert_eq!(rows(view).len(), 1);
    }

    #[test]
    fn view_is_a_message_while_scanning() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        let mut tools = WebTools::default();
        let project = host.invocation().project().unwrap();
        let view = tools.endpoints_view(ENDPOINT_VIEW, &project, Duration::ZERO);
        assert_eq!(message(&view), "Scanning…");
        assert_eq!(host.refreshed_views(), [ENDPOINT_VIEW]);
    }

    #[test]
    fn view_without_endpoints_is_a_message() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Plain.java", "class Plain {}");
        let mut tools = warmed();
        let view = tools.render(ENDPOINT_VIEW, None, &host.invocation()).unwrap();
        assert!(message(&view).contains("doesn't look like a Spring project"));
        host.add_file("pom.xml", "<project/>");
        let view = tools.render(ENDPOINT_VIEW, None, &host.invocation()).unwrap();
        assert_eq!(message(&view), NO_ENDPOINTS);
    }

    #[test]
    fn activating_an_endpoint_opens_its_location() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        let mut tools = warmed();
        let key = row_key(&host, &mut tools, "/api/users", "[POST] Users@create");
        tools.handle(ENDPOINT_VIEW, ViewEvent::Activate(key), &host.invocation()).unwrap();
        assert_eq!(host.opened_locations().len(), 1);
        assert_eq!(host.opened_locations()[0].0, "/project/src/main/java/com/acme/Users.java");
        assert_eq!(host.opened_locations()[0].1, 6);
    }

    fn curl_of(host: &MockHost, tools: &mut WebTools, path: &str, detail: &str) -> Option<String> {
        let key = row_key(host, tools, path, detail);
        tools.run_command("copy-endpoint-curl", &host.invocation(), Some(&key)).unwrap();
        host.clipboard()
    }

    #[test]
    fn curl_row_action_still_copies_the_old_text() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        let mut tools = warmed();
        host.answer_next_prompt(Some(&["http://localhost:9000/"]));
        assert_eq!(
            curl_of(&host, &mut tools, "/api/users", "[POST] Users@create").as_deref(),
            Some("curl -X POST 'http://localhost:9000/api/users'")
        );
    }

    #[test]
    fn curl_command_without_a_row_asks_for_the_endpoint() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        let mut tools = WebTools::default();
        let routes = tools.routes(&host.invocation().project().unwrap(), COMMAND_BUDGET).routes;
        let at = routes.iter().position(|r| r.method == "POST" && r.path == "/api/users").unwrap().to_string();
        host.answer_next_prompt(Some(&[at.as_str()]));
        host.answer_next_prompt(Some(&["http://localhost:9000"]));
        tools.run_command("copy-endpoint-curl", &host.invocation(), None).unwrap();
        assert_eq!(host.clipboard().as_deref(), Some("curl -X POST 'http://localhost:9000/api/users'"));
    }

    #[test]
    fn creates_an_http_file_for_an_endpoint() {
        let host = MockHost::install().with_capabilities(&["project.read", "project.write", "clipboard"]);
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        let mut tools = warmed();
        let key = row_key(&host, &mut tools, "/api/users", "[POST] Users@create");
        host.answer_next_prompt(Some(&["http://localhost:9000"]));
        tools.run_command("create-endpoint-http", &host.invocation(), Some(&key)).unwrap();
        assert_eq!(
            host::projects().unwrap()[0].read_file("http/post-api-users.http").unwrap(),
            "### POST /api/users\nPOST http://localhost:9000/api/users\n"
        );
        assert!(host.process_runs().is_empty());
        assert_eq!(host.opened_locations()[0].0, "/project/http/post-api-users.http");
    }

    #[test]
    fn create_http_request_file_denied_names_permission() {
        let host = MockHost::install().with_capabilities(&["project.read", "clipboard"]);
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        let mut tools = WebTools::default();
        let key = row_key(&host, &mut tools, "/api/users", "[POST] Users@create");
        host.answer_next_prompt(Some(&["http://localhost:9000"]));
        tools.run_command("create-endpoint-http", &host.invocation(), Some(&key)).unwrap();
        let notifications = host.notifications();
        assert!(host.clipboard().unwrap().contains("POST http://localhost:9000/api/users"));
        assert!(notifications.iter().any(|n| n.body.contains("needs the project.write permission")
            && n.body.contains("Settings › Plugins › Spring Endpoints Tools")));
    }

    #[test]
    fn copy_endpoint_curl_without_endpoints_explains() {
        let host = MockHost::install();
        let mut tools = WebTools::default();
        let error = tools.run_command("copy-endpoint-curl", &host.invocation(), None).unwrap_err();
        assert!(matches!(error, Error::Failed(ref message) if message == NO_ENDPOINTS), "{error:?}");
    }

    #[test]
    fn endpoint_picker_says_still_indexing_while_scanning() {
        assert!(matches!(no_routes_error(true), Error::Failed(ref message) if message == "Still indexing endpoints. Try again in a moment."));
        assert!(matches!(no_routes_error(false), Error::Failed(ref message) if message == NO_ENDPOINTS));
    }

    #[test]
    fn copy_as_curl_denied_names_permission() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        let mut tools = WebTools::default();
        let key = row_key(&host, &mut tools, "/api/users", "[POST] Users@create");
        host.answer_next_prompt(Some(&["http://localhost:9000/"]));
        tools.run_command("copy-endpoint-curl", &host.invocation(), Some(&key)).unwrap();
        let notifications = host.notifications();
        assert!(notifications.iter().any(|n| n.body.contains("needs the clipboard permission")
            && n.body.contains("Settings › Plugins › Spring Endpoints Tools")));
    }

    #[test]
    fn copy_openapi_export_denied_names_permission() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        let error = WebTools::default()
            .run_command("copy-endpoints-openapi", &host.invocation(), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("needs the clipboard permission"), "{error}");
        assert!(error.contains("Settings › Plugins › Spring Endpoints Tools"), "{error}");
    }

    #[test]
    fn dismissed_curl_prompt_copies_nothing() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        let mut tools = warmed();
        host.answer_next_prompt(None);
        assert_eq!(curl_of(&host, &mut tools, "/api/users", "[POST] Users@create"), None);
    }

    #[test]
    fn curl_without_clipboard_warns() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        let mut tools = warmed();
        host.answer_next_prompt(Some(&["http://h"]));
        assert_eq!(curl_of(&host, &mut tools, "/api/users", "[POST] Users@create"), None);
        assert_eq!(host.notifications().len(), 1);
    }

    #[test]
    fn malformed_key_is_ignored() {
        let host = MockHost::install();
        let mut tools = warmed();
        for key in ["route:x", "route:99", "e:1", ""] {
            tools.handle(ENDPOINT_VIEW, ViewEvent::Activate(key.into()), &host.invocation()).unwrap();
        }
        assert!(host.opened_locations().is_empty());
    }

    fn legacy_file_saved(host: &MockHost, tools: &mut WebTools) {
        host.add_file_bytes("src/main/java/com/acme/Legacy.java", b"class caf\xe9 {}");
        let saved = [openidea_plugin::Event::FileSaved("/project/src/main/java/com/acme/Legacy.java".into())];
        tools.endpoints.on_events(&saved, Instant::now() + COMMAND_BUDGET);
    }

    #[test]
    fn java_provider_cold_cache_returns_fast() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        for n in 0..30 {
            host.add_file(&format!("src/main/java/com/acme/S{n}.java"), &format!("package com.acme;\nclass S{n} {{}}\n"));
        }
        let mut tools = WebTools::default();
        let project = host.invocation().project().unwrap();
        let d = doc("src/main/java/com/acme/Users.java", API_CONTROLLER);
        let started = Instant::now();
        tools.line_markers(&d);
        tools.completion(&d, Position::new(5, 10), None);
        tools.definition(&d, Position::new(5, 10));
        assert!(started.elapsed() < Duration::from_millis(30));
        let cached = tools.endpoints.get(&project, Duration::ZERO).files().len();
        assert!(cached <= 1, "a provider on a cold cache read {cached} files");
    }

    #[test]
    fn refresh_resets_incomplete_index() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        let mut tools = warmed();
        let project = host.invocation().project().unwrap();
        tools.endpoints.get(&project, COMMAND_BUDGET);
        legacy_file_saved(&host, &mut tools);
        assert!(!tools.endpoints.get(&project, Duration::ZERO).complete());
        assert!(!tools.endpoints.get(&project, Duration::ZERO).scanning());
        host.add_file("src/main/java/com/acme/Legacy.java", "class Legacy {}");
        tools.handle(ENDPOINT_VIEW, ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
        assert!(tools.endpoints.get(&project, Duration::ZERO).files().is_empty(), "an incomplete index starts over");
        assert!(tools.endpoints.get(&project, COMMAND_BUDGET).complete());
    }

    #[test]
    fn partial_index_is_labelled_and_not_scanning() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        let mut tools = warmed();
        tools.render(ENDPOINT_VIEW, None, &host.invocation()).unwrap();
        legacy_file_saved(&host, &mut tools);
        let view = tools.render(ENDPOINT_VIEW, None, &host.invocation()).unwrap();
        assert!(view.status.unwrap().contains("partial index (1 unreadable)"));
    }

    #[test]
    fn refresh_keeps_a_scan_in_progress() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Other.java", "class Other {}");
        let mut tools = WebTools::default();
        let project = host.invocation().project().unwrap();
        tools.endpoints.get(&project, Duration::ZERO).update("/project/src/main/java/com/acme/Users.java", API_CONTROLLER);
        tools.handle(ENDPOINT_VIEW, ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
        assert_eq!(tools.endpoints.get(&project, Duration::ZERO).files().len(), 1, "not reset");
        assert_eq!(host.refreshed_views(), [ENDPOINT_VIEW]);

        tools.endpoints.get(&project, COMMAND_BUDGET);
        tools.handle(ENDPOINT_VIEW, ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
        assert!(tools.endpoints.get(&project, Duration::ZERO).files().is_empty(), "a complete index starts over");
    }

    #[test]
    fn view_ignores_dynamic_and_non_literal_mappings() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        let mut tools = warmed();
        let rows = rows(tools.render(ENDPOINT_VIEW, None, &host.invocation()).unwrap());
        assert!(rows.iter().all(|i| !i.label.contains("PATH")));
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn paths_with_colons_open_their_file() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Users.java", API_CONTROLLER);
        let mut tools = warmed();
        let key = row_key(&host, &mut tools, "/api/odd:name/{id}", "[GET] Users@odd");
        tools.handle(ENDPOINT_VIEW, ViewEvent::Activate(key), &host.invocation()).unwrap();
        assert_eq!(host.opened_locations()[0].0, "/project/src/main/java/com/acme/Users.java");
    }
}
