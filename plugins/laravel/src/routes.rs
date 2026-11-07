//! Routes of `routes/*.php`: `Route::get(…)->name(…)`, the legacy
//! `['as' => …]` option, `Route::resource` with `only`/`except`/`names`, and
//! the name and URI prefixes of the groups around them.

use std::ops::Range;

use crate::php::{self, is_identifier};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteDef {
    pub methods: String,
    pub uri: String,
    pub name: Option<String>,
    /// `UserController@index`, `Closure`, `view: welcome`.
    pub action: String,
    /// Short controller class and method, when the action names them.
    pub controller: Option<(String, String)>,
    /// The name when there is one, else the verb.
    pub offset: usize,
    pub width: usize,
}

const VERBS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "options", "any", "match", "view", "redirect", "permanentRedirect", "resource", "apiResource",
];
const RESOURCE_ACTIONS: &[(&str, &str, &str)] = &[
    ("index", "GET|HEAD", ""),
    ("create", "GET|HEAD", "/create"),
    ("store", "POST", ""),
    ("show", "GET|HEAD", "/{}"),
    ("edit", "GET|HEAD", "/{}/edit"),
    ("update", "PUT|PATCH", "/{}"),
    ("destroy", "DELETE", "/{}"),
];

struct Segment {
    name: String,
    start: usize,
    open: usize,
    args: Vec<Range<usize>>,
}

struct Chain {
    start: usize,
    segments: Vec<Segment>,
}

/// `Route::a(…)->b(…)->c(…)` starting at each `Route::` of the text.
fn chains(text: &str) -> Vec<Chain> {
    let bytes = text.as_bytes();

    text.match_indices("Route::")
        .filter(|(at, _)| *at == 0 || !(is_identifier(bytes[at - 1]) || bytes[at - 1] == b'$'))
        .filter_map(|(at, _)| parse_chain(text, at))
        .collect()
}

fn parse_chain(text: &str, at: usize) -> Option<Chain> {
    let mut i = at + "Route::".len();
    let mut segments = Vec::new();

    loop {
        let name_start = i;
        while i < text.len() && is_identifier(text.as_bytes()[i]) {
            i += 1;
        }
        if i == name_start {
            break;
        }

        let after = text[i..].trim_start();
        let open = text.len() - after.len();
        if !after.starts_with('(') {
            break;
        }

        let name = text[name_start..i].to_owned();
        let Some(close) = php::closing(text, open) else {
            segments.push(Segment { name, start: name_start, open, args: Vec::new() });
            break;
        };
        segments.push(Segment { name, start: name_start, open, args: php::split(text, open + 1..close) });

        let rest = text[close + 1..].trim_start();
        let Some(rest) = rest.strip_prefix("?->").or_else(|| rest.strip_prefix("->")) else { break };
        i = text.len() - rest.trim_start().len();
    }

    (!segments.is_empty()).then_some(Chain { start: at, segments })
}

struct Region {
    body: Range<usize>,
    names: String,
    prefix: String,
    controller: Option<String>,
}

fn short(class: &str) -> &str {
    class.rsplit('\\').next().unwrap_or(class)
}

fn join_uri(parts: &[&str]) -> String {
    let joined: Vec<&str> = parts.iter().map(|p| p.trim_matches('/')).filter(|p| !p.is_empty()).collect();
    format!("/{}", joined.join("/"))
}

fn first_literal<'a>(text: &'a str, segment: &Segment) -> Option<(&'a str, usize)> {
    php::literal(text, segment.args.first()?.clone())
}

fn controller_of(text: &str, range: Range<usize>) -> Option<String> {
    let class = php::class_constant(text, range.clone()).or_else(|| php::literal(text, range).map(|(c, _)| c))?;
    Some(short(class).to_owned())
}

/// The closure of a `group` call and what the chain before it adds.
fn region_of(text: &str, chain: &Chain) -> Option<Region> {
    let group = chain.segments.iter().find(|s| s.name == "group")?;
    let close = php::closing(text, group.open)?;
    let brace = group.open + text[group.open..close].find('{')?;
    let end = php::closing(text, brace)?;

    let mut names = String::new();
    let mut prefixes: Vec<&str> = Vec::new();
    let mut controller = None;

    for segment in chain.segments.iter().take_while(|s| s.start < group.start) {
        match segment.name.as_str() {
            "name" => names.extend(first_literal(text, segment).map(|(n, _)| n)),
            "prefix" => prefixes.extend(first_literal(text, segment).map(|(p, _)| p)),
            "controller" => controller = segment.args.first().and_then(|a| controller_of(text, a.clone())),
            _ => {}
        }
    }

    if let Some(options) = group.args.first().and_then(|a| php::array_open(text, a.clone())) {
        names.extend(php::string_value(text, options, "as").map(|(n, _)| n));
        prefixes.extend(php::string_value(text, options, "prefix").map(|(p, _)| p));
    }

    Some(Region { body: brace + 1..end, names, prefix: join_uri(&prefixes), controller })
}

/// The action shown for the route and the controller method it names.
fn action_of(text: &str, range: Option<Range<usize>>, controller: Option<&str>) -> (String, Option<(String, String)>) {
    let Some(range) = range else { return ("Closure".into(), None) };
    let trimmed = php::trimmed(text, range.clone());
    let source = &text[trimmed.clone()];

    if let Some((action, _)) = php::literal(text, range.clone()) {
        return match action.split_once('@') {
            Some((class, method)) => (action.to_owned(), Some((short(class).to_owned(), method.to_owned()))),
            None => match controller {
                Some(class) => (format!("{class}@{action}"), Some((class.to_owned(), action.to_owned()))),
                None => (action.to_owned(), None),
            },
        };
    }

    if let Some(class) = php::class_constant(text, range.clone()) {
        return (format!("{}@__invoke", short(class)), Some((short(class).to_owned(), "__invoke".into())));
    }

    if let Some(open) = php::array_open(text, range) {
        let items = php::items(text, open);
        if let Some(uses) = items.iter().find(|i| i.key.clone().and_then(|k| php::literal(text, k)).is_some_and(|(k, _)| k == "uses")) {
            return action_of(text, Some(uses.value.clone()), controller);
        }

        let class = items.first().and_then(|i| php::class_constant(text, i.value.clone()));
        let method = items.get(1).and_then(|i| php::literal(text, i.value.clone()));
        if let (Some(class), Some((method, _))) = (class, method) {
            return (format!("{}@{method}", short(class)), Some((short(class).to_owned(), method.to_owned())));
        }
    }

    if source.starts_with("function") || source.starts_with("fn") || source.starts_with("static") {
        return ("Closure".into(), None);
    }

    (source.chars().take(40).collect(), None)
}

fn strings_of(text: &str, range: &Range<usize>) -> Vec<String> {
    php::array_open(text, range.clone()).map(|open| php::strings(text, open, false)).unwrap_or_default()
}

/// The routes of one file, from its text with comments blanked.
pub fn parse(text: &str) -> Vec<RouteDef> {
    let chains = chains(text);
    let regions: Vec<Region> = chains.iter().filter_map(|chain| region_of(text, chain)).collect();
    let mut defs = Vec::new();

    for chain in &chains {
        let enclosing: Vec<&Region> = regions.iter().filter(|r| r.body.contains(&chain.start)).collect();
        defs.extend(route_defs(text, chain, &enclosing));
    }

    defs.sort_by_key(|d| d.offset);
    defs
}

fn route_defs(text: &str, chain: &Chain, enclosing: &[&Region]) -> Vec<RouteDef> {
    let Some(verb) = chain.segments.iter().find(|s| VERBS.contains(&s.name.as_str())) else { return Vec::new() };
    let name_prefix: String = enclosing.iter().map(|r| r.names.as_str()).collect();
    let controller = chain
        .segments
        .iter()
        .find(|s| s.name == "controller")
        .and_then(|s| s.args.first().and_then(|a| controller_of(text, a.clone())))
        .or_else(|| enclosing.iter().rev().find_map(|r| r.controller.clone()));

    let mut uri_parts: Vec<&str> = enclosing.iter().map(|r| r.prefix.as_str()).collect();
    uri_parts.extend(chain.segments.iter().filter(|s| s.name == "prefix").filter_map(|s| first_literal(text, s).map(|(p, _)| p)));

    let own_names: Vec<(&str, usize)> = chain.segments.iter().filter(|s| s.name == "name").filter_map(|s| first_literal(text, s)).collect();
    let arg = |i: usize| verb.args.get(i).cloned();
    let uri_literal = |i: usize| arg(i).and_then(|r| php::literal(text, r)).map_or("?", |(u, _)| u);

    let (methods, uri, options, action_range) = match verb.name.as_str() {
        "match" => {
            let methods = arg(0).map(|r| strings_of(text, &r)).unwrap_or_default().join("|").to_uppercase();
            (methods, uri_literal(1), arg(2), arg(2))
        }
        "any" => ("ANY".to_owned(), uri_literal(0), arg(1), arg(1)),
        "view" => ("GET|HEAD".to_owned(), uri_literal(0), None, None),
        "redirect" | "permanentRedirect" => ("GET|HEAD".to_owned(), uri_literal(0), None, None),
        "resource" | "apiResource" => return resource_defs(text, chain, verb, &name_prefix, &uri_parts, controller),
        name => (name.to_uppercase().replace("GET", "GET|HEAD"), uri_literal(0), arg(1), arg(1)),
    };

    let options_array = options.and_then(|r| php::array_open(text, r));
    let legacy_name = options_array.and_then(|open| php::string_value(text, open, "as"));
    let mut name: Option<String> = None;
    let mut offset = verb.start;
    let mut width = verb.name.len();

    if let Some((value, at)) = legacy_name {
        name = Some(value.to_owned());
        (offset, width) = (at, value.len());
    }

    if let Some((_, at)) = own_names.first() {
        let joined: String = own_names.iter().map(|(n, _)| *n).collect();
        name = Some(format!("{}{joined}", name.unwrap_or_default()));
        (offset, width) = (*at, own_names[0].0.len());
    }

    let (action, controller) = match verb.name.as_str() {
        "view" => (format!("view: {}", arg(1).and_then(|r| php::literal(text, r)).map_or("?", |(v, _)| v)), None),
        "redirect" | "permanentRedirect" => ("redirect".to_owned(), None),
        _ => action_of(text, action_range, controller.as_deref()),
    };

    uri_parts.push(uri);
    vec![RouteDef {
        methods,
        uri: join_uri(&uri_parts),
        name: name.map(|n| format!("{name_prefix}{n}")),
        action,
        controller,
        offset,
        width,
    }]
}

fn resource_defs(
    text: &str,
    chain: &Chain,
    verb: &Segment,
    name_prefix: &str,
    uri_parts: &[&str],
    group_controller: Option<String>,
) -> Vec<RouteDef> {
    let Some((resource, offset)) = verb.args.first().and_then(|r| php::literal(text, r.clone())) else { return Vec::new() };
    let controller = verb.args.get(1).and_then(|r| controller_of(text, r.clone())).or(group_controller);
    let api = verb.name == "apiResource";

    let options = verb.args.get(2).and_then(|r| php::array_open(text, r.clone()));
    let option = |name: &str| -> Option<usize> {
        let fluent = chain.segments.iter().find(|s| s.name == name).and_then(|s| s.args.first()).and_then(|r| php::array_open(text, r.clone()));
        fluent.or_else(|| {
            let item = php::items(text, options?).into_iter().find(|i| i.key.clone().and_then(|k| php::literal(text, k)).is_some_and(|(k, _)| k == name))?;
            php::array_open(text, item.value)
        })
    };
    let list = |name: &str| option(name).map(|open| php::strings(text, open, false));
    let only = list("only");
    let except = list("except");
    let renamed = option("names").map(|open| {
        php::items(text, open)
            .into_iter()
            .filter_map(|i| Some((php::literal(text, i.key?)?.0.to_owned(), php::literal(text, i.value)?.0.to_owned())))
            .collect::<Vec<_>>()
    });

    // `admin/photos` is named `photos`, `photos.comments` keeps its dot.
    let base_name = resource.rsplit('/').next().unwrap_or(resource);
    let parameter = base_name.rsplit('.').next().unwrap_or(base_name);
    let parameter = format!("{{{}}}", parameter.strip_suffix('s').unwrap_or(parameter));
    let base_uri = resource.replace('.', &format!("/{parameter}/"));

    RESOURCE_ACTIONS
        .iter()
        .filter(|(action, _, _)| !(api && matches!(*action, "create" | "edit")))
        .filter(|(action, _, _)| only.as_ref().is_none_or(|o| o.iter().any(|a| a == action)))
        .filter(|(action, _, _)| except.as_ref().is_none_or(|e| !e.iter().any(|a| a == action)))
        .map(|(action, methods, suffix)| {
            let name = renamed
                .as_ref()
                .and_then(|r| r.iter().find(|(a, _)| a == action).map(|(_, n)| n.clone()))
                .unwrap_or_else(|| format!("{base_name}.{action}"));
            let mut parts = uri_parts.to_vec();
            let path = format!("{base_uri}{}", suffix.replace("{}", &parameter));
            parts.push(&path);

            RouteDef {
                methods: (*methods).to_owned(),
                uri: join_uri(&parts),
                name: Some(format!("{name_prefix}{name}")),
                action: controller.as_deref().map_or_else(|| (*action).to_owned(), |c| format!("{c}@{action}")),
                controller: controller.clone().map(|c| (c, (*action).to_owned())),
                offset,
                width: resource.len(),
            }
        })
        .collect()
}

/// `use` lines of a routes file: the namespace each short class name is imported from.
pub fn imports(masked: &str) -> Vec<(String, String)> {
    masked
        .lines()
        .filter_map(|line| line.trim().strip_prefix("use ")?.strip_suffix(';'))
        .filter(|path| !path.contains(['{', ',', ' ']))
        .filter_map(|path| {
            let path = path.trim_start_matches('\\');
            let (namespace, class) = path.rsplit_once('\\')?;
            Some((class.to_owned(), namespace.to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(defs: &[RouteDef]) -> Vec<(&str, &str, &str)> {
        defs.iter().map(|d| (d.name.as_deref().unwrap_or("-"), d.uri.as_str(), d.action.as_str())).collect()
    }

    #[test]
    fn reads_named_routes_and_actions() {
        let text = "<?php\nRoute::get('/', fn () => 1)->name('home');\nRoute::post('/login', [AuthController::class, 'login'])->name('login');\nRoute::get('/old', ['as' => 'old', 'uses' => 'OldController@show']);\nRoute::view('/about', 'about')->name('about');\nRoute::get('/plain', 'PlainController@index');\n";
        let defs = parse(&crate::php::mask_comments(text));
        assert_eq!(
            names(&defs),
            [
                ("home", "/", "Closure"),
                ("login", "/login", "AuthController@login"),
                ("old", "/old", "OldController@show"),
                ("about", "/about", "view: about"),
                ("-", "/plain", "PlainController@index"),
            ]
        );
        assert_eq!(defs[1].methods, "POST");
        assert_eq!(defs[1].controller, Some(("AuthController".into(), "login".into())));
        assert_eq!(&text[defs[0].offset..defs[0].offset + defs[0].width], "home");
    }

    #[test]
    fn stacks_group_prefixes() {
        let text = "<?php\nRoute::prefix('admin')->name('admin.')->middleware('auth')->group(function () {\n    Route::get('/users', [UserController::class, 'index'])->name('users.index');\n    Route::name('reports.')->prefix('reports')->group(function () {\n        Route::get('/', fn () => 1)->name('home');\n    });\n});\nRoute::group(['as' => 'api.', 'prefix' => 'api'], function () {\n    Route::get('/ping', 'P@ping')->name('ping');\n});\nRoute::controller(PostController::class)->group(function () {\n    Route::get('/posts', 'index')->name('posts');\n});\n";
        let defs = parse(text);
        assert_eq!(
            names(&defs),
            [
                ("admin.users.index", "/admin/users", "UserController@index"),
                ("admin.reports.home", "/admin/reports", "Closure"),
                ("api.ping", "/api/ping", "P@ping"),
                ("posts", "/posts", "PostController@index"),
            ]
        );
    }

    #[test]
    fn expands_resources() {
        let text = "<?php\nRoute::resource('photos', PhotoController::class)->only(['index', 'show']);\nRoute::apiResource('admin/posts', 'PostController', ['except' => ['destroy']]);\nRoute::resource('comments', CommentController::class)->names(['index' => 'all.comments'])->only(['index']);\nRoute::name('v1.')->group(function () { Route::resource('tags', TagController::class)->only(['edit']); });\n";
        let defs = parse(text);
        let listed: Vec<_> = defs.iter().map(|d| (d.name.as_deref().unwrap(), d.methods.as_str(), d.uri.as_str())).collect();
        assert_eq!(
            listed,
            [
                ("photos.index", "GET|HEAD", "/photos"),
                ("photos.show", "GET|HEAD", "/photos/{photo}"),
                ("posts.index", "GET|HEAD", "/admin/posts"),
                ("posts.store", "POST", "/admin/posts"),
                ("posts.show", "GET|HEAD", "/admin/posts/{post}"),
                ("posts.update", "PUT|PATCH", "/admin/posts/{post}"),
                ("all.comments", "GET|HEAD", "/comments"),
                ("v1.tags.edit", "GET|HEAD", "/tags/{tag}/edit"),
            ]
        );
    }

    #[test]
    fn survives_half_typed_routes() {
        let text = "Route::prefix('a')->name('b.')->group(function () { Route::get('/x', [A::class, 'y'])->name('z'); Route::resource('r', R::class)->only(['index";
        for end in 0..=text.len() {
            parse(&text[..end]);
        }
    }

    #[test]
    fn reads_class_imports() {
        let imports = imports("<?php\nuse App\\Http\\Controllers\\PostController;\nuse Foo\\{A, B};\nuse Bar\\Baz as Q;\n");
        assert_eq!(imports, [("PostController".to_owned(), "App\\Http\\Controllers".to_owned())]);
    }
}
