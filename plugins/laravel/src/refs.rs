//! What the code around a position refers to. One forward pass over the
//! text keeps a stack of open brackets, so every string literal knows the
//! call it is an argument of (and the array key it is the value of).

use std::ops::Range;

use crate::php::{self, is_identifier, string_end};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Route,
    View,
    Config,
    Translation,
    Env,
    Asset,
    Middleware,
    /// `'UserController@index'`.
    Action,
    /// `'UserController'`, the controller of `Route::resource`.
    Controller,
    /// `'index'` in `[UserController::class, 'index']`.
    Method,
    Section,
    Stack,
    /// `<x-forms.input>`.
    Component,
    /// `<livewire:counter>`, `@livewire('counter')`.
    Livewire,
    /// `@vite('resources/js/app.js')`.
    Vite,
    /// `required` in `'name' => 'required|max:255'`.
    Rule,
    /// `'name'` in `User::where('name', …)`; `class` is the model, `table:<name>` for `DB::table()`, none for the model file.
    Column,
    /// `'update'` in `$user->can('update', $post)`.
    Ability,
    /// `'Posts/Index'` in `Inertia::render('Posts/Index')`.
    Inertia,
    /// `app('cache')`, `App::make(Foo::class)` strings, the class of `@inject('x', 'App\\Foo')`.
    Binding,
}

/// A name in the document and what kind of thing it names.
#[derive(Debug, PartialEq, Eq)]
pub struct Reference {
    pub kind: Kind,
    /// Byte range of the name in the document.
    pub start: usize,
    pub end: usize,
    /// The controller class of a `Method`.
    pub class: Option<String>,
    /// The string has its closing quote.
    pub closed: bool,
}

/// Where a string literal is: the call, the argument and the array key.
#[derive(Debug, Default)]
pub struct Site {
    /// `Route::get`, `->name`, `view`, `@include`; empty at top level.
    callee: String,
    callee_start: usize,
    arg: usize,
    /// Array literals between the argument and the string.
    depth: usize,
    /// Index of the string in its array.
    element: usize,
    /// The key the string is the value of.
    key: Option<String>,
    /// The key of the array the string's array is the value of.
    outer_key: Option<String>,
    array_open: Option<usize>,
}

const ROUTE_VERBS: &[&str] = &["get", "post", "put", "patch", "delete", "options", "any"];
/// Classes with static calls that are not Eloquent models.
pub const BUILDER_METHODS: &[&str] = &[
    "query", "where", "orWhere", "whereIn", "whereNotIn", "whereNull", "whereNotNull", "whereBetween", "whereDate", "whereHas", "whereDoesntHave",
    "has", "doesntHave", "with", "withCount", "withTrashed", "onlyTrashed", "orderBy", "orderByDesc", "latest", "oldest", "select", "addSelect",
    "distinct", "groupBy", "having", "limit", "take", "skip", "offset", "lockForUpdate", "withoutGlobalScopes", "newQuery",
];

const NOT_MODELS: &[&str] = &[
    "Route", "Schema", "Gate", "Auth", "Cache", "Config", "Log", "Storage", "Http", "Session", "Request", "Validator", "View", "Mail",
    "Lang", "URL", "Redirect", "Hash", "File", "Str", "Arr", "Cookie", "Collection", "Carbon", "Event", "Queue", "Bus", "Artisan",
    "Blade", "Vite", "Env", "Response", "Notification", "Password", "RateLimiter", "Process", "Number", "Sleep", "Date", "Crypt",
];
/// Query builder methods whose first argument is a column.
const COLUMN_METHODS: &[&str] = &[
    "where", "orWhere", "whereNot", "orWhereNot", "whereIn", "whereNotIn", "orWhereIn", "orWhereNotIn", "whereNull", "whereNotNull",
    "orWhereNull", "orWhereNotNull", "whereBetween", "whereNotBetween", "whereDate", "whereMonth", "whereDay", "whereYear", "whereTime",
    "orderBy", "orderByDesc", "latest", "oldest", "pluck", "value", "firstWhere", "max", "min", "sum", "avg", "count", "increment",
    "decrement",
];
/// Query builder methods that take columns in any argument.
const COLUMNS_METHODS: &[&str] = &["select", "addSelect", "groupBy"];
const COLUMN_PROPERTIES: &[&str] = &["$fillable", "$guarded", "$hidden", "$visible", "$casts", "$dates"];
const CONFIG_METHODS: &[&str] = &["get", "has", "set", "string", "integer", "float", "boolean", "array", "prepend", "push"];

impl Site {
    fn last_segment(&self) -> &str {
        self.callee.rsplit(['\\', '>', ':']).next().unwrap_or_default()
    }

    fn is_function(&self, names: &[&str]) -> bool {
        let plain = !self.callee.contains("->") && !self.callee.contains("::") && !self.callee.starts_with(['$', '@']);
        plain && names.contains(&self.last_segment())
    }

    fn is_method(&self, names: &[&str]) -> bool {
        self.callee.contains("->") && names.contains(&self.last_segment())
    }

    fn is_static(&self, classes: &[&str], names: &[&str]) -> bool {
        let Some((class, method)) = self.callee.rsplit_once("::") else { return false };
        let class = class.rsplit('\\').next().unwrap_or_default();
        classes.contains(&class) && names.contains(&method)
    }

    fn is_directive(&self, names: &[&str]) -> bool {
        self.callee.strip_prefix('@').is_some_and(|name| names.contains(&name))
    }

    /// The argument holding the rules array of a validation call.
    fn rules_argument(&self, text: &str) -> usize {
        let is_array = |range: Option<&Range<usize>>| {
            range.is_some_and(|r| {
                let argument = text[php::trimmed(text, r.clone())].trim_start();
                argument.starts_with('[') || argument.starts_with("array(")
            })
        };
        let arguments = || {
            let open = text[self.callee_start + self.callee.len()..].find('(')? + self.callee_start + self.callee.len();
            Some(php::arguments(text, open))
        };

        match self.callee.as_str() {
            "$this->validate" => {
                let arguments = arguments().unwrap_or_default();
                usize::from(!is_array(arguments.first()))
            }
            "$this->validateWithBag" => {
                let arguments = arguments().unwrap_or_default();
                if is_array(arguments.get(1)) { 1 } else { 2 }
            }
            _ if self.is_method(&["validateWithBag"]) || self.is_static(&["Validator"], &["make"]) || self.is_function(&["validator"]) => 1,
            _ => 0,
        }
    }

    /// The string is the value of a key in the rules array itself, not in a nested list or the messages array.
    fn is_rule_value(&self, text: &str) -> bool {
        self.depth == 1 && self.key.is_some() && self.arg == self.rules_argument(text)
    }

    /// The string is an element of the list of rules of a key of the rules array.
    fn is_rule_list_element(&self, text: &str) -> bool {
        self.depth == 2 && self.key.is_none() && self.outer_key.is_some() && self.arg == self.rules_argument(text)
    }

    fn direct(&self, argument: usize) -> bool {
        self.depth == 0 && self.arg == argument
    }

    /// The argument is a string or an array of strings.
    fn string_or_list(&self, argument: usize) -> bool {
        self.arg == argument && self.depth <= 1
    }
}

struct Frame {
    kind: FrameKind,
    open: usize,
    callee: Range<usize>,
    arg: usize,
    key: Option<String>,
    last_literal: Option<String>,
}

#[derive(PartialEq, Eq)]
enum FrameKind {
    Paren,
    Bracket,
    Brace,
}

pub struct Literal {
    /// The contents, between the quotes.
    pub start: usize,
    pub end: usize,
    pub closed: bool,
}

/// The identifier chain before the `(` at `open`: `$this->render`, `Route::get`, `\view`.
fn callee_before(bytes: &[u8], open: usize, lower: usize) -> Range<usize> {
    let mut end = open;
    while end > lower && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }

    let mut start = end;
    loop {
        while start > lower && (is_identifier(bytes[start - 1]) || matches!(bytes[start - 1], b'\\' | b'$' | b'@')) {
            start -= 1;
        }

        let joined = start >= lower + 2 && matches!(&bytes[start - 2..start], b"->" | b"::");
        if !joined {
            return start..end;
        }

        start -= 2;
        if start > lower && bytes[start - 1] == b'?' {
            start -= 1;
        }
    }
}

fn site_of(text: &str, stack: &[Frame]) -> Site {
    let Some(top) = stack.last() else { return Site::default() };

    let call = |frame: &Frame| (text[frame.callee.clone()].to_owned(), frame.callee.start, frame.arg);
    match top.kind {
        FrameKind::Brace => Site::default(),
        FrameKind::Paren => {
            let (callee, callee_start, arg) = call(top);
            Site { callee, callee_start, arg, key: top.key.clone(), ..Site::default() }
        }
        FrameKind::Bracket => {
            let parent = stack.len().checked_sub(2).and_then(|i| stack.get(i)).filter(|f| f.kind == FrameKind::Bracket);
            let outer_key = parent.and_then(|f| f.key.clone());
            let array = Site { element: top.arg, key: top.key.clone(), outer_key, array_open: Some(top.open), ..Site::default() };

            let mut depth = 1;
            for frame in stack.iter().rev().skip(1) {
                match frame.kind {
                    FrameKind::Bracket => depth += 1,
                    // An array in a function body, like `return [...]` in `rules()`.
                    FrameKind::Brace => return Site { depth, ..array },
                    FrameKind::Paren => {
                        let (callee, callee_start, arg) = call(frame);
                        return Site { callee, callee_start, arg, depth, ..array };
                    }
                }
            }
            Site { depth, ..array }
        }
    }
}

/// Visits every string literal of `from..text.len()` with its call site, until
/// `visit` returns false. Strings end at the line when they aren't closed
/// there, unless `multiline`.
fn walk(text: &str, from: usize, to: usize, comments: bool, multiline: bool, mut visit: impl FnMut(&Literal, &Site) -> bool) {
    let bytes = text.as_bytes();
    let mut stack: Vec<Frame> = Vec::new();
    let mut i = from;

    while i < to {
        let b = bytes[i];
        let next = bytes.get(i + 1).copied();

        match b {
            b'\'' | b'"' => {
                let mut end = string_end(bytes, i);
                if !multiline && let Some(newline) = text[i..end.min(bytes.len())].find('\n') {
                    end = i + newline;
                }
                let closed = end > i + 1 && bytes.get(end - 1) == Some(&b) && string_end(bytes, i) == end;
                let carriage_return = usize::from(!closed && end > i + 1 && bytes[end - 1] == b'\r');
                let literal = Literal { start: i + 1, end: if closed { end - 1 } else { end - carriage_return }, closed };

                if !visit(&literal, &site_of(text, &stack)) {
                    return;
                }

                if let Some(top) = stack.last_mut() {
                    top.last_literal = Some(text[literal.start..literal.end].to_owned());
                }
                i = end.max(i + 1);
                continue;
            }
            b'/' if comments && next == Some(b'/') => i = text[i..].find('\n').map_or(to, |n| i + n),
            b'#' if comments && next != Some(b'[') => i = text[i..].find('\n').map_or(to, |n| i + n),
            b'/' if comments && next == Some(b'*') => i = text[i + 2..].find("*/").map_or(to, |n| i + 2 + n + 2),
            b'(' => {
                let callee = callee_before(bytes, i, from);
                let is_array = text[callee.clone()].eq_ignore_ascii_case("array");
                let kind = if is_array { FrameKind::Bracket } else { FrameKind::Paren };
                stack.push(Frame { kind, open: i, callee, arg: 0, key: None, last_literal: None });
                i += 1;
            }
            b'[' => {
                stack.push(Frame { kind: FrameKind::Bracket, open: i, callee: i..i, arg: 0, key: None, last_literal: None });
                i += 1;
            }
            b'{' => {
                stack.push(Frame { kind: FrameKind::Brace, open: i, callee: i..i, arg: 0, key: None, last_literal: None });
                i += 1;
            }
            b')' | b']' | b'}' => {
                stack.pop();
                i += 1;
            }
            b',' => {
                if let Some(top) = stack.last_mut() {
                    top.arg += 1;
                    top.key = None;
                    top.last_literal = None;
                }
                i += 1;
            }
            b'=' if next == Some(b'>') => {
                if let Some(top) = stack.last_mut() {
                    top.key = top.last_literal.take();
                }
                i += 2;
            }
            b':' if next != Some(b':') && i > 0 && bytes[i - 1] != b':' => {
                if let Some(name) = named_argument(bytes, i, from)
                    && let Some(top) = stack.last_mut()
                    && top.kind == FrameKind::Paren
                {
                    top.key = Some(name.to_owned());
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
}

/// `name` of `name: value` right after `(` or `,`.
fn named_argument(bytes: &[u8], colon: usize, lower: usize) -> Option<&str> {
    let mut end = colon;
    while end > lower && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }

    let mut start = end;
    while start > lower && is_identifier(bytes[start - 1]) {
        start -= 1;
    }

    let mut before = start;
    while before > lower && bytes[before - 1].is_ascii_whitespace() {
        before -= 1;
    }

    let after_separator = before > lower && matches!(bytes[before - 1], b'(' | b',');
    (start < end && after_separator).then(|| std::str::from_utf8(&bytes[start..end]).ok()).flatten()
}

/// `Route::` appears shortly before, in the same statement.
fn in_route_chain(text: &str, before: usize) -> bool {
    let from = text.floor_char_boundary(before.saturating_sub(600));
    let window = &text[from..before];
    let statement = window.rfind(';').map_or(window, |at| &window[at..]);
    statement.contains("Route::")
}

/// The model a query on a column belongs to: `Some(Some("User"))`, `Some(Some("table:users"))`,
/// `Some(None)` for the model file the code is in, `None` when the string isn't a column.
fn column_target(text: &str, literal: &Literal, site: &Site) -> Option<Option<String>> {
    let method = site.last_segment();
    let called = site.callee.contains("->") || site.callee.contains("::");
    let column_argument = called && COLUMN_METHODS.contains(&method) && site.string_or_list(0)
        || called && COLUMNS_METHODS.contains(&method) && site.depth <= 1;

    if column_argument {
        return query_target(text, site);
    }

    // `protected $fillable = ['name']`, `protected $casts = ['price' => 'float']`.
    let open = site.array_open?;
    if !site.callee.is_empty() || site.depth != 1 {
        return None;
    }

    let property = text[..open].trim_end().strip_suffix('=')?.trim_end();
    let property = COLUMN_PROPERTIES.iter().find(|p| property.ends_with(*p))?;
    let is_key = text[literal.end..].trim_start_matches(['\'', '"']).trim_start().starts_with("=>");
    (is_key == (*property == "$casts")).then_some(None)
}

/// The model of the query chain a call is part of.
fn query_target(text: &str, site: &Site) -> Option<Option<String>> {
    let class_of = |word: &str| -> Option<Option<String>> {
        let word = word.rsplit('\\').next().unwrap_or(word);
        if word.starts_with(|c: char| c.is_ascii_lowercase()) {
            return Some(None);
        }
        (!NOT_MODELS.contains(&word)).then(|| Some(word.to_owned()))
    };

    if let Some((class, _)) = site.callee.rsplit_once("::") {
        return class_of(class);
    }

    let from = text.floor_char_boundary(site.callee_start.saturating_sub(600));
    let window = &text[from..site.callee_start];
    let statement = window.rfind([';', '{', '}']).map_or(window, |at| &window[at + 1..]);

    if let Some(at) = statement.find("DB::table(") {
        let after = &statement[at + "DB::table(".len()..];
        let table = after.trim_start().strip_prefix(['\'', '"']).and_then(|rest| rest.split(['\'', '"']).next());
        return table.map(|t| Some(format!("table:{t}")));
    }

    for (at, _) in statement.match_indices("::") {
        let word_start = statement[..at].rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '\\')).map_or(0, |i| i + 1);
        let word = &statement[word_start..at];
        if word.rsplit('\\').next().is_some_and(|short| short.starts_with(|c: char| c.is_ascii_uppercase())) {
            return class_of(word).map(|class| class.map(|name| with_hops(&name, &hops_after_static(&statement[at + 2..]))));
        }
    }

    if let Some(at) = statement.rfind("$this->") {
        let hops = hops_after(&statement[at + "$this->".len()..]);
        if !hops.is_empty() {
            return Some(Some(with_hops("", &hops)));
        }
    }

    Some(None)
}

fn with_hops(model: &str, hops: &[String]) -> String {
    hops.iter().fold(model.to_owned(), |spec, hop| format!("{spec}>{hop}"))
}

/// Method names of the `->a()->b(` chain that follows the static call at the start of `rest`.
fn hops_after_static(rest: &str) -> Vec<String> {
    let name_length = rest.bytes().take_while(|b| is_identifier(*b)).count();
    let after = &rest[name_length..];
    let Some(open) = after.starts_with('(').then_some(name_length) else { return Vec::new() };
    let Some(close) = php::closing(rest, open) else { return Vec::new() };
    hops_after_arrow(&rest[close + 1..])
}

/// Method names of `a()->b(` where `rest` starts at `a`.
fn hops_after(rest: &str) -> Vec<String> {
    hops_in(rest).into_iter().filter(|hop| !BUILDER_METHODS.contains(&hop.as_str())).collect()
}

fn hops_in(rest: &str) -> Vec<String> {
    let mut hops = Vec::new();
    let mut remaining = rest;

    loop {
        let length = remaining.bytes().take_while(|b| is_identifier(*b)).count();
        if length == 0 || !remaining[length..].starts_with('(') {
            break;
        }

        hops.push(remaining[..length].to_owned());
        let Some(close) = php::closing(remaining, length) else { break };
        let Some(next) = remaining[close + 1..].trim_start().strip_prefix("->") else { break };
        remaining = next.trim_start();
    }

    hops
}

fn hops_after_arrow(rest: &str) -> Vec<String> {
    let trimmed = rest.trim_start();
    trimmed.strip_prefix("?->").or_else(|| trimmed.strip_prefix("->")).map(|next| hops_after(next.trim_start())).unwrap_or_default()
}

/// The nearest function before `at` is called `rules`: a form request.
fn in_rules_method(text: &str, at: usize) -> bool {
    let Some(found) = text[..at].rfind("function ") else { return false };
    let name = &text[found + "function ".len()..];
    name.starts_with("rules") && !name["rules".len()..].bytes().next().is_some_and(is_identifier)
}

fn after_redirect_helper(text: &str, before: usize) -> bool {
    text[..before].trim_end().ends_with("redirect()")
}

fn classify(text: &str, language: &str, literal: &Literal, site: &Site) -> Option<(Kind, Option<String>)> {
    let direct = |i| site.direct(i);
    let blade = language == "blade";
    let route_call = site.is_static(&["Route"], ROUTE_VERBS) || (site.is_method(ROUTE_VERBS) && in_route_chain(text, site.callee_start));
    let route_resource_call = site.is_static(&["Route"], &["resource", "apiResource"])
        || (site.is_method(&["resource", "apiResource"]) && in_route_chain(text, site.callee_start));
    let chained = !blade && site.callee.contains("->") && in_route_chain(text, site.callee_start);

    if site.is_function(&["route", "to_route", "link_to_route"]) && direct(0)
        || site.is_static(&["Redirect", "URL"], &["route", "signedRoute", "temporarySignedRoute"]) && direct(0)
        || site.is_static(&["Route"], &["has", "is", "currentRouteNamed"]) && site.string_or_list(0)
        || site.is_method(&["signedRoute", "temporarySignedRoute", "routeIs", "linkRoute", "visitRoute", "toRoute"]) && site.string_or_list(0)
        || site.is_method(&["route"]) && direct(0) && after_redirect_helper(text, site.callee_start)
    {
        return Some((Kind::Route, None));
    }

    let content_view = site.callee.ends_with("Content") && matches!(site.key.as_deref(), Some("view" | "markdown" | "text" | "html"));
    let route_view = (site.is_static(&["Route"], &["view"]) || site.is_method(&["view"]) && chained) && direct(1);
    let plain_view = site.is_method(&["view", "markdown"]) && !chained && direct(0);
    if site.is_function(&["view"]) && direct(0)
        || site.is_static(&["View"], &["make", "exists", "first"]) && site.string_or_list(0)
        || site.is_static(&["Mail"], &["send", "queue", "later"]) && direct(0)
        || content_view && site.depth == 0
        || route_view
        || plain_view
        || site.is_directive(&["extends", "include", "includeIf", "component"]) && direct(0)
        || site.is_directive(&["includeWhen", "includeUnless"]) && direct(1)
        || site.is_directive(&["includeFirst"]) && site.string_or_list(0)
        || site.is_directive(&["each"]) && (direct(0) || direct(3))
    {
        return Some((Kind::View, None));
    }

    if site.is_function(&["config"]) && direct(0) || site.is_static(&["Config"], CONFIG_METHODS) && direct(0) {
        return Some((Kind::Config, None));
    }

    if site.is_function(&["env"]) && direct(0) || site.is_static(&["Env"], &["get", "getOrFail"]) && direct(0) {
        return Some((Kind::Env, None));
    }

    if site.is_function(&["__", "trans", "trans_choice"]) && direct(0)
        || site.is_static(&["Lang"], &["get", "has", "choice", "hasForLocale"]) && direct(0)
        || site.is_directive(&["lang", "choice"]) && direct(0)
    {
        return Some((Kind::Translation, None));
    }

    if site.is_function(&["asset", "secure_asset", "secureAsset"]) && direct(0) || site.is_static(&["URL"], &["asset", "secureAsset"]) && direct(0)
    {
        return Some((Kind::Asset, None));
    }

    let middleware_call = site.is_method(&["middleware", "withoutMiddleware"]) || site.is_static(&["Route"], &["middleware", "withoutMiddleware"]);
    if middleware_call && site.string_or_list(0) || site.key.as_deref() == Some("middleware") || site.outer_key.as_deref() == Some("middleware") {
        return Some((Kind::Middleware, None));
    }

    let method_literal = (route_call && site.arg == 1 || site.is_function(&["action"]) && site.arg == 0)
        && site.depth == 1
        && site.element == 1
        && site.key.is_none();
    if method_literal {
        let class = site.array_open.and_then(|open| {
            let first = php::split(text, open + 1..literal.start.saturating_sub(1)).into_iter().next()?;
            php::class_constant(text, first).map(str::to_owned)
        });
        return class.map(|class| (Kind::Method, Some(class)));
    }

    if route_call && (direct(1) || site.arg == 1 && site.depth == 1 && site.key.as_deref() == Some("uses"))
        || site.is_static(&["Route"], &["match"]) && direct(2)
        || site.is_function(&["action", "link_to_action"]) && direct(0)
        || site.is_static(&["Redirect", "URL"], &["action"]) && direct(0)
    {
        return Some((Kind::Action, None));
    }

    if route_resource_call && direct(1) {
        return Some((Kind::Controller, None));
    }

    if (site.is_function(&["app", "resolve"]) || site.is_static(&["App"], &["make", "makeWith", "get", "bound", "resolved", "isShared"])) && direct(0)
        || site.is_directive(&["inject"]) && direct(1)
    {
        return Some((Kind::Binding, None));
    }

    if let Some(class) = column_target(text, literal, site) {
        return Some((Kind::Column, class));
    }

    if site.is_directive(&["vite"]) && site.string_or_list(0) || site.is_static(&["Vite"], &["asset"]) && direct(0) {
        return Some((Kind::Vite, None));
    }

    if site.is_directive(&["livewire"]) && direct(0) {
        return Some((Kind::Livewire, None));
    }

    if site.is_static(&["Inertia"], &["render"]) && direct(0) || site.is_function(&["inertia"]) && direct(0) || site.is_static(&["Route"], &["inertia"]) && direct(1)
    {
        return Some((Kind::Inertia, None));
    }

    if (site.is_method(&["can", "cannot", "cant", "authorize"]) || site.is_static(&["Gate"], &["allows", "denies", "check", "authorize", "inspect"]))
        && direct(0)
        || site.is_static(&["Gate"], &["any", "none"]) && site.string_or_list(0)
        || site.is_directive(&["can", "cannot", "elsecan", "elsecannot"]) && direct(0)
        || site.is_directive(&["canany", "elsecanany"]) && site.string_or_list(0)
    {
        return Some((Kind::Ability, None));
    }

    let validating = site.is_method(&["validate", "validateWithBag"]) || site.is_static(&["Validator"], &["make"]) || site.is_function(&["validator"]);
    let rule_shape = site.depth == 1 && site.key.is_some() || site.depth == 2 && site.key.is_none() && site.outer_key.is_some();
    if rule_shape && (validating || site.callee.is_empty() && in_rules_method(text, literal.start)) {
        return Some((Kind::Rule, None));
    }

    if site.is_directive(&["section", "yield", "hasSection", "sectionMissing"]) && direct(0) {
        return Some((Kind::Section, None));
    }

    if site.is_directive(&["push", "pushOnce", "prepend", "prependOnce", "stack"]) && direct(0) || site.is_directive(&["pushIf"]) && direct(1) {
        return Some((Kind::Stack, None));
    }

    None
}

fn reference(text: &str, language: &str, literal: &Literal, site: &Site, at: Option<usize>) -> Option<Reference> {
    let (kind, class) = classify(text, language, literal, site)?;
    let contents = &text[literal.start..literal.end];
    let mut start = literal.start;

    let end = match kind {
        // `throttle:60,1` names the `throttle` middleware.
        Kind::Middleware => literal.start + contents.find(':').unwrap_or(contents.len()),
        // `required|max:255`: the rule under the cursor, but not its parameters.
        Kind::Rule if at.is_some() => {
            let relative = at?.saturating_sub(literal.start).min(contents.len());
            let from = contents[..relative].rfind('|').map_or(0, |i| i + 1);
            if contents[from..relative].contains(':') {
                return None;
            }
            start = literal.start + from;
            literal.start + relative + contents[relative..].find(['|', ':']).unwrap_or(contents.len() - relative)
        }
        _ => literal.end,
    };

    Some(Reference { kind, start, end, class, closed: literal.closed })
}

/// Where to start reading to know the context of `at`.
fn window_start(language: &str, text: &str, at: usize) -> Option<usize> {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);

    if language == "blade" {
        let line = &text[line_start..at];
        let echo = line.rfind("{{").into_iter().chain(line.rfind("{!!")).max();
        let directive = line.match_indices('@').map(|(i, _)| i).filter(|&i| i == 0 || !is_identifier(line.as_bytes()[i - 1])).max();
        return echo.into_iter().chain(directive).max().map(|i| line_start + i);
    }

    let lower = text.floor_char_boundary(at.saturating_sub(4000));
    let statement = text[lower..at].rfind(';').map_or(lower, |i| lower + i + 1);
    Some(statement)
}

/// What the name at byte offset `at` refers to, if anything.
pub fn reference_at(language: &str, text: &str, at: usize) -> Option<Reference> {
    if language == "blade"
        && let Some(tag) = tag_reference(text, at)
    {
        return Some(tag);
    }

    let from = window_start(language, text, at)?;
    let mut found = None;

    walk(text, from, text.len(), language != "blade", false, |literal, site| {
        if literal.start > at {
            return false;
        }
        if at <= literal.end {
            found = reference(text, language, literal, site, Some(at));
            return false;
        }
        true
    });

    found
}

/// The whole string literal at `at` and what it holds, for the actions that rewrite it.
pub fn literal_at(language: &str, text: &str, at: usize) -> Option<(Kind, Range<usize>)> {
    let from = window_start(language, text, at)?;
    let mut found = None;

    walk(text, from, text.len(), language != "blade", false, |literal, site| {
        if literal.start > at {
            return false;
        }
        if at <= literal.end {
            if literal.closed {
                found = classify(text, language, literal, site)
                    .filter(|(kind, _)| *kind != Kind::Rule || site.is_rule_value(text))
                    .map(|(kind, _)| (kind, literal.start..literal.end));
            }
            return false;
        }
        true
    });

    found
}

/// The bracket opening the list of rules whose element is at `at`: `['required', 'email']` of `'email' => [...]`.
pub fn rule_list_at(language: &str, text: &str, at: usize) -> Option<usize> {
    let from = window_start(language, text, at)?;
    let mut found = None;

    walk(text, from, text.len(), language != "blade", false, |literal, site| {
        if literal.start > at {
            return false;
        }
        if at <= literal.end {
            if literal.closed && matches!(classify(text, language, literal, site), Some((Kind::Rule, _))) && site.is_rule_list_element(text) {
                found = site.array_open;
            }
            return false;
        }
        true
    });

    found
}

/// The component or Livewire name of `<x-name` / `<livewire:name` at `at`.
fn tag_reference(text: &str, at: usize) -> Option<Reference> {
    let bytes = text.as_bytes();
    let allowed = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_');

    let mut start = at;
    while start > 0 && allowed(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = at;
    while end < bytes.len() && allowed(bytes[end]) {
        end += 1;
    }

    // `x-` is part of the word scanned above, `livewire:` is not.
    let is_component = text[start..end].starts_with("x-") && (text[..start].ends_with('<') || text[..start].ends_with("</"));
    let kind = if is_component {
        start += 2;
        Kind::Component
    } else if text[..start].ends_with("<livewire:") || text[..start].ends_with("</livewire:") {
        Kind::Livewire
    } else {
        return None;
    };

    Some(Reference { kind, start, end, class: None, closed: true })
}

/// Blade and HTML comments blanked out, so the byte offsets stay the same.
fn without_comments(text: &str) -> String {
    let mut out = text.as_bytes().to_vec();

    for (open, close) in [("{{--", "--}}"), ("<!--", "-->")] {
        let mut from = 0;
        while let Some(start) = text[from..].find(open).map(|n| from + n) {
            let end = text[start..].find(close).map_or(text.len(), |n| start + n + close.len());
            for b in &mut out[start..end] {
                if *b != b'\n' {
                    *b = b' ';
                }
            }
            from = end;
        }
    }

    String::from_utf8(out).unwrap_or_default()
}

/// Every `<x-name` and `<livewire:name` of a Blade document, except the ones
/// that live in a package namespace (`<x-package::name>`) or aren't names
/// (`<x-slot:title>`, `<x-{{ $name }}>`).
pub fn tag_references(text: &str) -> Vec<Reference> {
    let masked = without_comments(text);
    let bytes = masked.as_bytes();
    let mut found = Vec::new();

    for (prefix, kind) in [("<x-", Kind::Component), ("<livewire:", Kind::Livewire)] {
        for (at, _) in masked.match_indices(prefix) {
            let start = at + prefix.len();
            let end = (start..bytes.len()).find(|&i| !(bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'.' | b'-' | b'_'))).unwrap_or(bytes.len());

            if end == start || bytes.get(end) == Some(&b':') || bytes.get(end) == Some(&b'{') {
                continue;
            }

            found.push(Reference { kind, start, end, class: None, closed: true });
        }
    }

    found.sort_by_key(|r| r.start);
    found
}

/// The class of every `Class::factory(` call.
pub fn factory_classes(text: &str) -> Vec<Range<usize>> {
    let masked = php::mask_comments(text);
    let bytes = masked.as_bytes();
    let mut found = Vec::new();

    for (at, _) in masked.match_indices("::factory(") {
        let start = (0..at).rev().find(|&i| !(is_identifier(bytes[i]) || bytes[i] == b'\\')).map_or(0, |i| i + 1);
        let before = start.checked_sub(1).map(|i| bytes[i]);

        if start < at && !matches!(before, Some(b'$' | b'>' | b':')) {
            found.push(start..at);
        }
    }

    found
}

/// Ranges of a Blade document that are code: echoes, directive arguments.
pub fn blade_regions(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut regions = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let end = match bytes[i] {
            b'{' if bytes[i..].starts_with(b"{{--") => text[i + 4..].find("--}}").map_or(bytes.len(), |n| i + 4 + n + 4),
            b'{' if bytes[i..].starts_with(b"{{") => text[i..].find("}}").map_or(bytes.len(), |n| i + n + 2),
            b'{' if bytes[i..].starts_with(b"{!!") => text[i..].find("!!}").map_or(bytes.len(), |n| i + n + 3),
            b'@' if i == 0 || !is_identifier(bytes[i - 1]) => {
                let name_end = (i + 1..bytes.len()).find(|&j| !bytes[j].is_ascii_alphabetic()).unwrap_or(bytes.len());
                let after = text[name_end..].trim_start_matches([' ', '\t']);
                let open = text.len() - after.len();
                if name_end > i + 1 && after.starts_with('(') {
                    php::closing(text, open).map_or(text[open..].find('\n').map_or(bytes.len(), |n| open + n), |c| c + 1)
                } else {
                    i + 1
                }
            }
            _ => i + 1,
        };

        if end > i + 1 && !text[i..end].starts_with("{{--") {
            regions.push(i..end);
        }
        i = end.max(i + 1);
    }

    regions
}

/// Every reference in a document, in order.
pub fn references(language: &str, text: &str) -> Vec<Reference> {
    let mut found = Vec::new();
    let mut collect = |literal: &Literal, site: &Site| {
        found.extend(reference(text, language, literal, site, None));
        true
    };

    if language == "blade" {
        for region in blade_regions(text) {
            walk(text, region.start, region.end, false, false, &mut collect);
        }
    } else {
        walk(text, 0, text.len(), true, true, &mut collect);
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind_at(language: &str, marked: &str) -> Option<(Kind, String)> {
        let at = marked.find('|').unwrap();
        let text = marked.replacen('|', "", 1);
        reference_at(language, &text, at).map(|r| (r.kind, text[r.start..r.end].to_owned()))
    }

    /// Like `kind_at` with `^` as the cursor, for texts that contain `|`.
    fn kind_at_caret(language: &str, marked: &str) -> Option<(Kind, String)> {
        let at = marked.find('^').unwrap();
        let text = marked.replacen('^', "", 1);
        reference_at(language, &text, at).map(|r| (r.kind, text[r.start..r.end].to_owned()))
    }

    fn some(kind: Kind, name: &str) -> Option<(Kind, String)> {
        Some((kind, name.into()))
    }

    #[test]
    fn detects_routes_views_config_translations_and_env() {
        assert_eq!(kind_at("php", "return redirect()->route('users.|show', $u);"), some(Kind::Route, "users.show"));
        assert_eq!(kind_at("php", "$url = route(\"hom|"), some(Kind::Route, "hom"));
        assert_eq!(kind_at("php", "Redirect::route('a|')"), some(Kind::Route, "a"));
        assert_eq!(kind_at("php", "request()->routeIs(['admin.*', 'us|ers.index'])"), some(Kind::Route, "users.index"));
        assert_eq!(kind_at("php", "$request->route('us|er')"), None);
        assert_eq!(kind_at("php", "return view('posts.in|dex', ['a' => 'b']);"), some(Kind::View, "posts.index"));
        assert_eq!(kind_at("php", "View::first(['a', 'b|'])"), some(Kind::View, "b"));
        assert_eq!(kind_at("php", "Route::view('/about', 'ab|out');"), some(Kind::View, "about"));
        assert_eq!(kind_at("php", "Route::view('/ab|out', 'about');"), None);
        assert_eq!(kind_at("php", "return $this->view('emails.w|elcome');"), some(Kind::View, "emails.welcome"));
        assert_eq!(kind_at("php", "new Content(view: 'mail.o|rder')"), some(Kind::View, "mail.order"));
        assert_eq!(kind_at("php", "config('app.na|me')"), some(Kind::Config, "app.name"));
        assert_eq!(kind_at("php", "Config::get('database.|')"), some(Kind::Config, "database."));
        assert_eq!(kind_at("php", "env('APP_|KEY', 'x')"), some(Kind::Env, "APP_KEY"));
        assert_eq!(kind_at("php", "env('APP_KEY', 'x|')"), None);
        assert_eq!(kind_at("php", "__('messages.wel|come')"), some(Kind::Translation, "messages.welcome"));
        assert_eq!(kind_at("php", "Lang::get('auth.fa|iled')"), some(Kind::Translation, "auth.failed"));
        assert_eq!(kind_at("php", "asset('css/a|pp.css')"), some(Kind::Asset, "css/app.css"));
        assert_eq!(kind_at("php", "$this->get('/x|')"), None);
        assert_eq!(kind_at("php", "$x = 'it\\'s |here';"), None);
    }

    #[test]
    fn detects_middleware_and_controllers() {
        assert_eq!(kind_at("php", "Route::get('/', fn() => 1)->middleware('thro|ttle:60,1');"), some(Kind::Middleware, "throttle"));
        assert_eq!(kind_at("php", "Route::middleware(['auth', 'ver|ified'])->group(function () {"), some(Kind::Middleware, "verified"));
        assert_eq!(kind_at("php", "Route::group(['middleware' => ['au|th']], function () {"), some(Kind::Middleware, "auth"));
        assert_eq!(kind_at("php", "$this->middleware('au|th');"), some(Kind::Middleware, "auth"));
        assert_eq!(kind_at("php", "Route::get('/users', 'UserController@in|dex');"), some(Kind::Action, "UserController@index"));
        assert_eq!(kind_at("php", "Route::get('/users', ['uses' => 'UserController@in|dex']);"), some(Kind::Action, "UserController@index"));
        assert_eq!(kind_at("php", "Route::post('/users', 'Us|"), some(Kind::Action, "Us"));
        assert_eq!(kind_at("php", "Route::prefix('a')->get('/users', 'U|')"), some(Kind::Action, "U"));
        assert_eq!(kind_at("php", "Route::resource('photos', 'Photo|Controller');"), some(Kind::Controller, "PhotoController"));
        assert_eq!(kind_at("php", "$client->get('/users', 'U|')"), None);

        let text = "Route::get('/users', [UserController::class, 'in']);";
        let at = text.find("'in'").unwrap() + 2;
        let reference = reference_at("php", text, at).unwrap();
        assert_eq!((reference.kind, reference.class.as_deref()), (Kind::Method, Some("UserController")));
    }

    #[test]
    fn detects_blade_directives_and_echoes() {
        assert_eq!(kind_at("blade", "@extends('layouts.a|pp')"), some(Kind::View, "layouts.app"));
        assert_eq!(kind_at("blade", "<div>@include('parti|als.nav', ['a' => 1])</div>"), some(Kind::View, "partials.nav"));
        assert_eq!(kind_at("blade", "@includeWhen($x, 'a|b')"), some(Kind::View, "ab"));
        assert_eq!(kind_at("blade", "@includeFirst(['a', 'b|'])"), some(Kind::View, "b"));
        assert_eq!(kind_at("blade", "@each('item', $items, 'i', 'em|pty')"), some(Kind::View, "empty"));
        assert_eq!(kind_at("blade", "<a href=\"{{ route('ho|me') }}\">"), some(Kind::Route, "home"));
        assert_eq!(kind_at("blade", "{{ __('Welc|ome') }} {!! config('app.name') !!}"), some(Kind::Translation, "Welcome"));
        assert_eq!(kind_at("blade", "{{ $a }} don't {{ asset('x|') }}"), some(Kind::Asset, "x"));
        assert_eq!(kind_at("blade", "@section('ti|tle', 'Home')"), some(Kind::Section, "title"));
        assert_eq!(kind_at("blade", "@yield('con|tent')"), some(Kind::Section, "content"));
        assert_eq!(kind_at("blade", "@push('scr|ipts')"), some(Kind::Stack, "scripts"));
        assert_eq!(kind_at("blade", "@lang('messages.wel|come')"), some(Kind::Translation, "messages.welcome"));
        assert_eq!(kind_at("blade", "<p>don't view('a|')</p>"), None);
    }

    #[test]
    fn detects_components_vite_abilities_and_rules() {
        assert_eq!(kind_at("blade", "<div><x-forms.in|put :a=\"1\" /></div>"), some(Kind::Component, "forms.input"));
        assert_eq!(kind_at("blade", "<x-|"), some(Kind::Component, ""));
        assert_eq!(kind_at("blade", "</x-ale|rt>"), some(Kind::Component, "alert"));
        assert_eq!(kind_at("blade", "<livewire:coun|ter />"), some(Kind::Livewire, "counter"));
        assert_eq!(kind_at("blade", "@livewire('co|unter')"), some(Kind::Livewire, "counter"));
        assert_eq!(kind_at("blade", "<div class=\"x-|\">"), None);
        assert_eq!(kind_at("blade", "@vite(['resources/css/a|pp.css', 'resources/js/app.js'])"), some(Kind::Vite, "resources/css/app.css"));
        assert_eq!(kind_at("blade", "@vite('res|')"), some(Kind::Vite, "res"));
        assert_eq!(kind_at("php", "$user->can('upd|ate', $post)"), some(Kind::Ability, "update"));
        assert_eq!(kind_at("php", "Gate::allows('vi|ew')"), some(Kind::Ability, "view"));
        assert_eq!(kind_at("php", "Gate::any(['a', 'b|'])"), some(Kind::Ability, "b"));
        assert_eq!(kind_at("php", "$this->authorize('del|ete', $post);"), some(Kind::Ability, "delete"));
        assert_eq!(kind_at("blade", "@can('edi|t', $post)"), some(Kind::Ability, "edit"));
        assert_eq!(kind_at("blade", "@canany(['a', 'b|'])"), some(Kind::Ability, "b"));
        assert_eq!(kind_at("php", "$user->can($x, 'po|st')"), None);

        assert_eq!(kind_at_caret("php", "$request->validate(['name' => 'req^uired|max:255']);"), some(Kind::Rule, "required"));
        assert_eq!(kind_at_caret("php", "$request->validate(['name' => 'required|ma^x:255']);"), some(Kind::Rule, "max"));
        assert_eq!(kind_at_caret("php", "$request->validate(['name' => 'required|max:2^55']);"), None);
        assert_eq!(kind_at_caret("php", "$request->validate(['name' => 'required|^']);"), some(Kind::Rule, ""));
        assert_eq!(kind_at_caret("php", "$request->validate(['name' => 'required|e^|max']);"), some(Kind::Rule, "e"));
        assert_eq!(kind_at("php", "$this->validate($request, ['tags' => ['required', 'ar|']]);"), some(Kind::Rule, "ar"));
        assert_eq!(kind_at("php", "Validator::make($data, ['a' => 'req|']);"), some(Kind::Rule, "req"));
        assert_eq!(
            kind_at("php", "class R {\n public function rules(): array\n {\n  return ['name' => 'req|'];\n }\n}"),
            some(Kind::Rule, "req")
        );
        assert_eq!(kind_at("php", "$x = ['name' => 'req|'];"), None);
    }

    #[test]
    fn detects_columns_and_their_model() {
        let target = |language: &str, marked: &str| {
            let at = marked.find('|').unwrap();
            let text = marked.replacen('|', "", 1);
            reference_at(language, &text, at).filter(|r| r.kind == Kind::Column).map(|r| (text[r.start..r.end].to_owned(), r.class))
        };
        let model = |name: &str, class: &str| Some((name.to_owned(), Some(class.to_owned())));

        assert_eq!(target("php", "User::where('na|me', 'x')"), model("name", "User"));
        assert_eq!(target("php", "return \\App\\Models\\User::query()->with('posts')->orderBy('cre|ated_at');"), model("created_at", "User"));
        assert_eq!(target("php", "Post::select('id', 'ti|tle')->get();"), model("title", "Post"));
        assert_eq!(target("php", "Post::whereIn('ida|', [1])"), model("ida", "Post"));
        assert_eq!(target("php", "DB::table('users')->where('na|me', 1)"), model("name", "table:users"));
        assert_eq!(target("php", "$query->where('ac|tive', 1)"), Some(("active".into(), None)));
        assert_eq!(target("php", "static::where('ac|tive', 1)"), Some(("active".into(), None)));
        assert_eq!(target("php", "User::where('name', 'Jo|e')"), None);
        assert_eq!(target("php", "Route::get('/x', 'A@b')->where('i|d', '[0-9]+')"), None);
        assert_eq!(target("php", "Arr::pluck($users, 'na|me')"), None);
        assert_eq!(target("php", "class U {\n protected $fillable = ['na|me', 'email'];\n}"), Some(("name".into(), None)));
        assert_eq!(target("php", "class U {\n protected $casts = ['pri|ce' => 'float'];\n}"), Some(("price".into(), None)));
        assert_eq!(target("php", "class U {\n protected $casts = ['price' => 'fl|oat'];\n}"), None);
        assert_eq!(target("php", "class U {\n protected $other = ['na|me'];\n}"), None);
    }

    #[test]
    fn finds_every_reference_of_a_document() {
        let php = "<?php\n// view('commented');\nview('a.b', ['x' => route('c')]);\n$s = \"multi\nline\";\nconfig('d');";
        let found: Vec<_> = references("php", php).into_iter().map(|r| (r.kind, &php[r.start..r.end])).collect();
        assert_eq!(found, [(Kind::View, "a.b"), (Kind::Route, "c"), (Kind::Config, "d")]);

        let blade = "@extends('l')\n{{-- @include('no') --}}\n<p>{{ route('r') }} @lang('t')</p>\n";
        let found: Vec<_> = references("blade", blade).into_iter().map(|r| (r.kind, &blade[r.start..r.end])).collect();
        assert_eq!(found, [(Kind::View, "l"), (Kind::Route, "r"), (Kind::Translation, "t")]);
    }

    #[test]
    fn survives_every_prefix() {
        let php = "<?php\nRoute::get('/é', [A::class, 'é'])->name(\"x\")->middleware(['é', fn() => view('ü'), array('k' => 'v']);\n$a = f(b: 'é');";
        let blade = "@if(route('é'))\n{{ __('ü', [ 'a' => \"b\" ]) }}\n@include(\n<p>don't @endif {!! $é !!}";
        for (language, text) in [("php", php), ("blade", blade)] {
            for (end, _) in text.char_indices().chain([(text.len(), ' ')]) {
                let prefix = &text[..end];
                references(language, prefix);
                for at in 0..=prefix.len() {
                    if prefix.is_char_boundary(at) {
                        reference_at(language, prefix, at);
                    }
                }
            }
        }
    }
}
