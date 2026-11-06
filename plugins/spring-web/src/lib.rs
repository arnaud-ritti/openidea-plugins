//! Spring Web Tools for OpenIDEA: checks `@PathVariable` parameters against
//! the URI templates of their request mappings, and links the view names a
//! `@Controller` returns to their template files, and links functional routes
//! and test URLs to their handlers. Text-based.

use endpoints::{FileInfo, Handler};
use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::{offset, position};
use openidea_plugin::{
    Diagnostic, Error, Invocation, Location, Plugin, Position, Range, Severity, TextEdit, host, register_plugin,
};
use spring_text::index::{Index, Indexes, PROVIDER_BUDGET, project_of};
use spring_text::java::{self, Annotation, JavaFile, Param, Str};
use spring_text::{SOURCE_GLOBS, config, contains, is_kotlin, range};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

mod checks;
mod endpoints;
mod generate;

pub(crate) const MAPPINGS: [&str; 6] =
    ["RequestMapping", "GetMapping", "PostMapping", "PutMapping", "DeleteMapping", "PatchMapping"];
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
const VERSIONING_GLOBS: &[&str] =
    &["**/*.java", "**/*.kt", "**/application*.properties", "**/application*.yml", "**/application*.yaml"];
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
            let in_paths: Vec<Vec<String>> =
                paths.iter().map(|p| variables(p).into_iter().map(|v| v.0).collect()).collect();
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
            let unclaimed: Vec<&String> = in_paths
                .first()
                .into_iter()
                .flatten()
                .filter(|v| !claimed.contains(v) && in_paths.iter().all(|p| p.contains(v)))
                .collect();
            for (a, name) in bound {
                let Some(i) = in_paths.iter().position(|p| !p.contains(&name)) else { continue };
                let message = format!("`{name}` is not a variable of the mapped path `{}`.", paths[i]);
                let fix = match unclaimed.as_slice() {
                    [only] => Some((
                        format!("Bind the `{{{only}}}` variable"),
                        TextEdit { range: range(text, a.name_at, a.end), text: format!("PathVariable(\"{only}\")") },
                    )),
                    _ => None,
                };
                let diagnostic =
                    Diagnostic::new(range(text, a.at, a.end), Severity::Error, message).code("unknown-path-variable");
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
    for class in file.types.iter().filter(|t| t.has(&["Controller"]) && !t.has(&["RestController", "ResponseBody"])) {
        for m in class.methods() {
            if (m.has(&MAPPINGS) || m.has(&["ExceptionHandler"]))
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
    settings: Indexes<Vec<(String, String)>>,
    versioning: Indexes<checks::Versioning>,
    /// Glob → when listed and the files.
    listings: BTreeMap<String, (Instant, Vec<String>)>,
}

impl Default for WebTools {
    fn default() -> Self {
        Self {
            endpoints: Indexes::new(SOURCE_GLOBS, endpoints::file_info),
            settings: Indexes::new(CONFIG_GLOBS, view_settings),
            versioning: Indexes::new(VERSIONING_GLOBS, checks::versioning),
            listings: BTreeMap::new(),
        }
    }
}

impl WebTools {
    /// The findings of the text checks over a Java document.
    fn checked(&mut self, doc: &Document) -> Vec<Diagnostic> {
        if is_kotlin(&doc.path) {
            return Vec::new();
        }
        let groups = checks::Groups::from_settings();
        let (configured, standard_parser) = match self.versioning.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) {
            Some(index) if groups.api_versioning || groups.version_format => {
                let files: Vec<_> = index.iter().map(|(_, v)| *v).collect();
                (checks::project_versioning(index.complete(), &files), checks::project_standard_parser(index.complete(), &files))
            }
            _ => (None, None),
        };
        checks::check_java(&java::parse(&doc.text), configured, standard_parser, groups)
            .iter()
            .map(|f| f.diagnostic(&doc.text))
            .collect()
    }

    /// The project's templates, those of the document's module first.
    fn templates(&mut self, doc: &Document) -> Vec<Template> {
        let Some(project) = project_of(&doc.path) else { return Vec::new() };
        let mut settings = BTreeMap::new();
        if let Some(index) = self.settings.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) {
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
                    let Some(i) = path.find(&format!("/{dir}")) else { continue };
                    let Some(name) = path[i + 1 + dir.len()..].strip_suffix(&format!(".{ext}")) else { continue };
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
        let Some(index) = self.endpoints.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else {
            return Vec::new();
        };
        if !index.complete() {
            return Vec::new();
        }
        let index = &*index;
        let Some(mine) = index.files().get(&doc.path) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for r in endpoints::requests(&doc.path, &doc.text) {
            let targets: Vec<Location> = index
                .iter()
                .flat_map(|(p, info)| {
                    info.endpoints
                        .iter()
                        .filter(|e| endpoints::serves(e, &r))
                        .map(move |e| Location { path: p.to_owned(), range: e.range })
                })
                .collect();
            if !targets.is_empty() {
                out.push(Link { at: r.at, end: r.end, tooltip: format!("Handler of {} {}", r.verb, r.path), targets });
            }
        }
        for e in &mine.endpoints {
            let Some(handler) = &e.handler else { continue };
            let targets = handler_targets(index, &doc.path, handler);
            if !targets.is_empty() {
                out.push(Link {
                    at: handler.at,
                    end: handler.end,
                    tooltip: format!("Handler of {} {}", e.verb, e.path),
                    targets,
                });
            }
        }
        out
    }

    /// The view literals of the document and the templates each names.
    fn views(&mut self, doc: &Document) -> Vec<(Str, Vec<Template>)> {
        if is_kotlin(&doc.path) {
            return Vec::new();
        }
        let literals = view_literals(&doc.text, &java::parse(&doc.text));
        if literals.is_empty() {
            return Vec::new();
        }
        let templates = self.templates(doc);
        literals
            .into_iter()
            .map(|s| {
                let found: Vec<Template> =
                    templates.iter().filter(|t| t.name == view_name(&s.value)).cloned().collect();
                (s, found)
            })
            .collect()
    }
}

fn problems(doc: &Document) -> Vec<Problem> {
    if is_kotlin(&doc.path) {
        return Vec::new();
    }
    path_problems(&doc.text, &java::parse(&doc.text))
}

impl Providers for WebTools {
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let mut diagnostics: Vec<Diagnostic> = problems(doc).into_iter().map(|p| p.diagnostic).collect();
        diagnostics.extend(self.checked(doc));
        let _ = host::publish_diagnostics(&doc.path, &diagnostics);
        let links = self.links(doc).into_iter().map(|l| {
            let others = if l.targets.len() > 1 { format!(" (+{} more)", l.targets.len() - 1) } else { String::new() };
            LineMarker {
                line: position(&doc.text, l.at).line,
                icon: None,
                tooltip: format!("{}{others}", l.tooltip),
                target: l.targets.first().cloned(),
            }
        });
        let views = self
            .views(doc)
            .into_iter()
            .filter_map(|(s, found)| {
                let first = found.first()?;
                let others = if found.len() > 1 { format!(" (+{} more)", found.len() - 1) } else { String::new() };
                Some(LineMarker {
                    line: position(&doc.text, s.at).line,
                    icon: None,
                    tooltip: format!("View template {}{others}", first.short()),
                    target: Some(first.location()),
                })
            })
            .collect::<Vec<_>>();
        views.into_iter().chain(links).collect()
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let mut out: Vec<Location> = self
            .views(doc)
            .into_iter()
            .filter(|(s, _)| s.holds(at))
            .flat_map(|(_, found)| found.into_iter().map(|t| t.location()))
            .collect();
        out.extend(self.links(doc).into_iter().filter(|l| l.at <= at && at <= l.end).flat_map(|l| l.targets));
        out
    }

    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(offset) = offset(&doc.text, at) else { return Vec::new() };
        if is_kotlin(&doc.path) {
            return Vec::new();
        }
        let literals = view_literals(&doc.text, &java::parse(&doc.text));
        let Some(s) = literals.iter().find(|s| s.holds(offset)) else { return Vec::new() };
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

impl Plugin for WebTools {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

register_plugin!(WebTools, providers);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

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
        let host = MockHost::install();
        let mut tools = WebTools::default();
        let d = doc("src/main/java/com/acme/web/PetApi.java", API);
        tools.line_markers(&d);
        let problems = &host.diagnostics()["/project/src/main/java/com/acme/web/PetApi.java"];
        let found: Vec<_> =
            problems.iter().map(|p| (p.code.as_deref().unwrap(), p.range.start.line, p.message.as_str())).collect();
        assert_eq!(
            found,
            [
                (
                    "unknown-path-variable",
                    6,
                    "`id` is not a variable of the mapped path `/owners/{ownerId}/pets/{petId}`."
                ),
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
        host.add_file(
            "src/main/resources/application.properties",
            "spring.mvc.view.prefix=/WEB-INF/pages/\nspring.mvc.view.suffix=.jsp\n",
        );
        host
    }

    #[test]
    fn finds_view_names_of_controller_methods() {
        let file = java::parse(CONTROLLER);
        let names: Vec<_> = view_literals(CONTROLLER, &file).into_iter().map(|s| s.value).collect();
        assert_eq!(names, ["users/list", "users/show", "fragments/card :: card", "home"]);
    }

    #[test]
    fn links_view_names_to_templates() {
        let _host = web_project();
        let mut tools = WebTools::default();
        let d = doc("src/main/java/com/acme/web/UserController.java", CONTROLLER);
        let markers = tools.line_markers(&d);
        let found: Vec<_> = markers.iter().map(|m| (m.line, m.tooltip.as_str())).collect();
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
        assert_eq!(
            targets,
            [Location {
                path: "/project/src/main/resources/templates/users/list.html".into(),
                range: Range::default()
            }]
        );
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
        let mut tools = WebTools::default();
        let d = doc("src/main/java/com/acme/Routes.java", JAVA_ROUTES);
        let markers = tools.line_markers(&d);
        let found: Vec<_> =
            markers.iter().map(|m| (m.line, m.target.as_ref().map(|t| t.path.rsplit('/').next().unwrap()))).collect();
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

    const USER_TESTS: &str = r#"@WebMvcTest
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
        host.add_file("src/main/java/com/acme/UserApi.java", USERS);
        let mut tools = WebTools::default();
        let d = doc("src/test/java/com/acme/UserApiTests.java", USER_TESTS);
        let markers = tools.line_markers(&d);
        let found: Vec<_> = markers.iter().map(|m| (m.line, m.target.as_ref().unwrap().range.start.line)).collect();
        assert_eq!(found, [(7, 7), (8, 10), (10, 4), (11, 7), (12, 12), (13, 10)]);
        assert_eq!(markers[0].tooltip, "Handler of GET /users/{id}");

        let at = Position::new(7, 30);
        let targets = tools.definition(&d, at);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, "/project/src/main/java/com/acme/UserApi.java");
        assert_eq!(targets[0].range.start.line, 7);
        assert!(tools.definition(&d, Position::new(9, 30)).is_empty(), "no POST /users mapping");
        let main = doc("src/main/java/com/acme/UserApi.java", USERS);
        assert!(tools.line_markers(&main).is_empty(), "not a test source");
    }

    #[test]
    fn completes_view_names() {
        let _host = web_project();
        let mut tools = WebTools::default();
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
        let host = MockHost::install();
        host.add_file("src/main/resources/application.properties", "server.port=1\n");
        let path = "src/main/java/com/acme/web/Api.java";
        let broken = "package a;\nimport org.springframework.web.bind.annotation.*;\n@RestController\nclass Api {\n    @GetMapping(path = \"/é😀\", version = \"2\")\n    String a() { return \"\"; }\n    @InitBinder\n    String b(WebDataBinder d) { return \"\"; }\n}\n";
        let mut tools = WebTools::default();
        tools.line_markers(&doc(path, broken));
        let key = format!("/project/{path}");
        let found = &host.diagnostics()[&key];
        let codes: Vec<_> = found.iter().filter_map(|d| d.code.as_deref()).collect();
        assert_eq!(codes, ["api-version-mismatch", "init-binder-signature"]);
        assert_eq!(found[0].range.start.line, 4);
        assert_eq!(found[0].range.end.character - found[0].range.start.character, 3);

        let fixed = broken.replace(", version = \"2\"", "").replace("String b(", "void b(").replace("return \"\"; }\n}", "}\n}");
        tools.line_markers(&doc(path, &fixed));
        assert!(host.diagnostics().get(&key).is_none_or(|d| d.is_empty()));
    }
}
