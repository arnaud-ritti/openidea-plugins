use std::collections::BTreeSet;

use openidea_plugin::host::{self, storage};
use openidea_plugin::views::{Body, Button, ListItem, TreeItem, View, ViewEvent};
use openidea_plugin::{Error, Location, NotificationLevel, PickItem};

const MAX_ROWS: usize = 5000;
const MAX_KEY_BYTES: usize = 256;
const FILTER_BUTTON: &str = "routes:filter";
const CLEAR_BUTTON: &str = "routes:clear";
const MODE_BUTTON: &str = "routes:mode";
const ROUTE_PREFIX: &str = "route:";
const GROUP_PREFIX: &str = "controller:";
const TITLE: &str = "Routes";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handler {
    pub class: String,
    pub member: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub method: String,
    pub path: String,
    pub handler: Handler,
    pub name: Option<String>,
    pub location: Option<Location>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteEvent {
    Ignored,
    Changed,
    Open(Route),
}

pub struct RouteList {
    mode_key: String,
    filter_key: String,
    plugin_id: String,
    tree: bool,
    filter: BTreeSet<String>,
    routes: Vec<Route>,
}

impl RouteList {
    pub fn new(storage_key: &str, plugin_id: &str) -> RouteList {
        let mode_key = format!("{storage_key}.mode");
        let filter_key = format!("{storage_key}.filter");
        let tree = storage::get(&mode_key).ok().flatten().as_deref() == Some("tree");
        let filter = storage::get(&filter_key)
            .ok()
            .flatten()
            .map(|stored| stored.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect())
            .unwrap_or_default();
        RouteList { mode_key, filter_key, plugin_id: plugin_id.to_owned(), tree, filter, routes: Vec::new() }
    }

    pub fn set_routes(&mut self, routes: Vec<Route>) {
        self.routes = routes;
        let known = self.classes();
        let before = self.filter.len();
        self.filter.retain(|class| known.contains(class));
        if self.filter.len() != before {
            self.store_filter();
        }
    }

    pub fn route(&self, key: &str) -> Option<&Route> {
        let index: usize = key.strip_prefix(ROUTE_PREFIX)?.parse().ok()?;
        self.routes.get(index)
    }

    pub fn render(&self, view: View) -> View {
        let mut view = view;
        if self.routes.is_empty() {
            view.body = Body::Message("No routes found in this project.".into());
            view.status = None;
            return view;
        }

        let shown: Vec<(usize, &Route)> = self
            .routes
            .iter()
            .enumerate()
            .filter(|(_, route)| self.filter.is_empty() || self.filter.contains(&route.handler.class))
            .collect();
        let total = shown.len();
        let capped = &shown[..total.min(MAX_ROWS)];

        view.body = if self.tree { self.tree_body(capped) } else { self.list_body(capped) };
        view.status = Some(self.status_line(capped.len(), total));
        view.toolbar.push(Button::new(FILTER_BUTTON, "Filter\u{2026}").icon("filter"));
        if !self.filter.is_empty() {
            view.toolbar.push(Button::new(CLEAR_BUTTON, "Clear"));
        }
        view.toolbar.push(Button::new(MODE_BUTTON, if self.tree { "Flat" } else { "Tree" }));
        view
    }

    pub fn handle(&mut self, event: &ViewEvent) -> RouteEvent {
        match event {
            ViewEvent::Button(id) if id == FILTER_BUTTON => self.pick_filter(),
            ViewEvent::Button(id) if id == CLEAR_BUTTON => {
                self.filter.clear();
                self.store_filter();
                RouteEvent::Changed
            }
            ViewEvent::Button(id) if id == MODE_BUTTON => {
                self.tree = !self.tree;
                let _ = storage::set(&self.mode_key, if self.tree { "tree" } else { "flat" });
                RouteEvent::Changed
            }
            ViewEvent::Activate(key) => self.route(key).cloned().map_or(RouteEvent::Ignored, RouteEvent::Open),
            _ => RouteEvent::Ignored,
        }
    }

    fn pick_filter(&mut self) -> RouteEvent {
        let items: Vec<PickItem> = self
            .classes()
            .iter()
            .filter(|class| class.len() <= MAX_KEY_BYTES)
            .take(MAX_ROWS)
            .map(|class| PickItem::new(class, class))
            .collect();
        match host::pick_many("Filter by controller", &items) {
            Ok(Some(chosen)) => {
                self.filter = chosen.into_iter().collect();
                self.store_filter();
                RouteEvent::Changed
            }
            Ok(None) | Err(_) => RouteEvent::Ignored,
        }
    }

    fn classes(&self) -> BTreeSet<String> {
        self.routes.iter().map(|route| route.handler.class.clone()).collect()
    }

    fn store_filter(&self) {
        let joined = self.filter.iter().cloned().collect::<Vec<_>>().join("\n");
        let _ = storage::set(&self.filter_key, &joined);
    }

    fn status_line(&self, shown: usize, total: usize) -> String {
        let mut line = if shown < total {
            format!("showing {shown} of {total} endpoints")
        } else {
            format!("{total} endpoints")
        };
        if !self.filter.is_empty() {
            let classes: Vec<&str> = self.filter.iter().map(String::as_str).collect();
            line.push_str(&format!(" \u{b7} controller: {}", classes.join(", ")));
        }
        line
    }

    fn icon(&self, method: &str) -> String {
        let name = match method.to_ascii_uppercase().as_str() {
            "GET" => "get",
            "POST" => "post",
            "PUT" => "put",
            "DELETE" => "delete",
            "PATCH" => "patch",
            _ => "other",
        };
        format!("ext:fileIcon/{}/route-{name}", self.plugin_id)
    }

    fn detail(route: &Route) -> String {
        let method = route.method.to_ascii_uppercase();
        match &route.name {
            Some(name) => format!("[{method}] {} \u{b7} {name}", route.handler.class),
            None => format!("[{method}] {}", route.handler.class),
        }
    }

    fn list_body(&self, rows: &[(usize, &Route)]) -> Body {
        Body::List(
            rows.iter()
                .map(|(index, route)| {
                    ListItem::new(&format!("{ROUTE_PREFIX}{index}"), &route.path)
                        .icon(&self.icon(&route.method))
                        .detail(&Self::detail(route))
                })
                .collect(),
        )
    }

    fn tree_body(&self, rows: &[(usize, &Route)]) -> Body {
        let mut items = Vec::new();
        let mut classes: Vec<&str> = rows.iter().map(|(_, route)| route.handler.class.as_str()).collect();
        classes.sort();
        classes.dedup();
        for class in classes {
            let group = format!("{GROUP_PREFIX}{class}");
            items.push(TreeItem::new(&group, class).icon("folder").expandable(true));
            for (index, route) in rows.iter().filter(|(_, route)| route.handler.class == class) {
                items.push(
                    TreeItem::new(&format!("{ROUTE_PREFIX}{index}"), &route.path)
                        .parent(&group)
                        .icon(&self.icon(&route.method))
                        .detail(&Self::detail(route)),
                );
            }
        }
        Body::Tree(items)
    }
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn url(base: &str, path: &str) -> String {
    format!("{}{path}", base.trim_end_matches('/'))
}

pub fn curl(method: &str, base: &str, path: &str) -> String {
    format!("curl -X {} {}", method.to_ascii_uppercase(), shell_quote(&url(base, path)))
}

pub fn http_request(method: &str, base: &str, path: &str) -> String {
    let method = method.to_ascii_uppercase();
    format!("### {method} {path}\n{method} {}\n", url(base, path))
}

fn ask_base(default_base: &str) -> Result<Option<String>, Error> {
    let answer = host::input("Base URL", Some(default_base))?;
    Ok(answer.map(|base| base.trim().to_owned()).filter(|base| !base.is_empty()))
}

pub fn copy_as_curl(route: &Route, default_base: &str) -> Result<(), Error> {
    let Some(base) = ask_base(default_base)? else {
        return Ok(());
    };
    host::copy_to_clipboard(&curl(&route.method, &base, &route.path))
}

fn http_file_name(route: &Route) -> String {
    let slug: String = route
        .path
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let slug = slug.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-");
    let slug = if slug.is_empty() { "root" } else { &slug };
    format!("{}-{slug}.http", route.method.to_ascii_lowercase())
}

/// `directory` is where the file goes; `None` means `<project root>/http`.
pub fn create_http_file(route: &Route, default_base: &str, directory: Option<&str>) -> Result<(), Error> {
    let Some(base) = ask_base(default_base)? else {
        return Ok(());
    };
    let project = host::projects()?.into_iter().next().ok_or_else(|| Error::NotFound("project".into()))?;
    let name = http_file_name(route);
    let (relative, path) = match directory {
        Some(directory) => {
            let path = format!("{}/{name}", directory.trim_end_matches('/'));
            (path.clone(), path)
        }
        None => (format!("http/{name}"), format!("{}/http/{name}", project.root()?.trim_end_matches('/'))),
    };
    if project.read_file(&relative).is_ok() {
        return host::open_location(&path, 0, 0);
    }

    let content = http_request(&route.method, &base, &route.path);
    let written = host::run_process("tee", &[path.clone()], Some(&content))
        .and_then(|out| if out.status == Some(0) { Ok(()) } else { Err(Error::Failed(out.stderr)) });
    match written {
        Ok(()) => host::open_location(&path, 0, 0),
        Err(Error::Denied(_)) => {
            host::copy_to_clipboard(&content)?;
            host::notify(
                NotificationLevel::Warning,
                TITLE,
                &format!("Could not write {relative}; the request is on the clipboard. Allow running tee in Settings \u{203a} Plugins to create the file."),
            );
            Ok(())
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::ProcessOutput;
    use openidea_plugin::mock::MockHost;

    const ID: &str = "aritti.demo";

    fn route(method: &str, path: &str, class: &str) -> Route {
        Route {
            method: method.into(),
            path: path.into(),
            handler: Handler { class: class.into(), member: None },
            name: None,
            location: None,
        }
    }

    fn list(routes: Vec<Route>) -> RouteList {
        let mut list = RouteList::new("routes", ID);
        list.set_routes(routes);
        list
    }

    fn base_view() -> View {
        View::new(Body::Message(String::new()))
    }

    fn items(view: &View) -> &[ListItem] {
        match &view.body {
            Body::List(items) => items,
            other => panic!("expected a list, got {other:?}"),
        }
    }

    #[test]
    fn flat_rows_show_path_label_and_grey_method_and_controller() {
        let _host = MockHost::install();
        let mut named = route("get", "/users/{id}", "UserController");
        named.name = Some("app_user_show".into());
        let view = list(vec![named, route("POST", "/login", "AuthController")]).render(base_view());
        let rows = items(&view);
        assert_eq!(rows[0].key, "route:0");
        assert_eq!(rows[0].label, "/users/{id}");
        assert_eq!(rows[0].detail.as_deref(), Some("[GET] UserController \u{b7} app_user_show"));
        assert_eq!(rows[1].detail.as_deref(), Some("[POST] AuthController"));
        assert_eq!(view.status.as_deref(), Some("2 endpoints"));
    }

    #[test]
    fn rows_carry_one_icon_per_method() {
        let _host = MockHost::install();
        let methods = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD"];
        let routes = methods.iter().map(|m| route(m, "/x", "C")).collect();
        let view = list(routes).render(base_view());
        let icons: Vec<&str> = items(&view).iter().map(|i| i.icon.as_deref().unwrap()).collect();
        let expected: Vec<String> = ["get", "post", "put", "delete", "patch", "other"]
            .iter()
            .map(|n| format!("ext:fileIcon/{ID}/route-{n}"))
            .collect();
        assert_eq!(icons, expected.iter().map(String::as_str).collect::<Vec<_>>());
    }

    #[test]
    fn filter_limits_rows_and_shows_in_the_status_line() {
        let host = MockHost::install();
        let mut list = list(vec![route("GET", "/a", "A"), route("GET", "/b", "B"), route("GET", "/c", "A")]);
        host.answer_next_prompt(Some(&["A"]));
        assert_eq!(list.handle(&ViewEvent::Button(FILTER_BUTTON.into())), RouteEvent::Changed);
        let view = list.render(base_view());
        assert_eq!(items(&view).len(), 2);
        assert_eq!(view.status.as_deref(), Some("2 endpoints \u{b7} controller: A"));
        assert!(view.toolbar.iter().any(|b| b.id == CLEAR_BUTTON));
        assert_eq!(RouteList::new("routes", ID).filter, BTreeSet::from(["A".to_owned()]));

        list.handle(&ViewEvent::Button(CLEAR_BUTTON.into()));
        let view = list.render(base_view());
        assert_eq!(items(&view).len(), 3);
        assert!(view.toolbar.iter().all(|b| b.id != CLEAR_BUTTON));
    }

    #[test]
    fn dismissed_filter_prompt_keeps_the_filter() {
        let host = MockHost::install();
        let mut list = list(vec![route("GET", "/a", "A"), route("GET", "/b", "B")]);
        host.answer_next_prompt(Some(&["B"]));
        list.handle(&ViewEvent::Button(FILTER_BUTTON.into()));
        host.answer_next_prompt(None);
        assert_eq!(list.handle(&ViewEvent::Button(FILTER_BUTTON.into())), RouteEvent::Ignored);
        assert_eq!(items(&list.render(base_view())).len(), 1);
    }

    #[test]
    fn vanished_controller_is_dropped_from_the_filter() {
        let host = MockHost::install();
        let mut list = list(vec![route("GET", "/a", "A"), route("GET", "/b", "B")]);
        host.answer_next_prompt(Some(&["B"]));
        list.handle(&ViewEvent::Button(FILTER_BUTTON.into()));
        list.set_routes(vec![route("GET", "/a", "A")]);
        let view = list.render(base_view());
        assert_eq!(items(&view).len(), 1);
        assert_eq!(view.status.as_deref(), Some("1 endpoints"));
        assert_eq!(host.storage().get("routes.filter").map(String::as_str), Some(""));
    }

    #[test]
    fn no_routes_shows_an_empty_message() {
        let _host = MockHost::install();
        let view = list(Vec::new()).render(base_view());
        assert!(matches!(view.body, Body::Message(ref text) if text.contains("No routes")));
    }

    #[test]
    fn tree_mode_groups_by_controller() {
        let host = MockHost::install();
        let mut list = list(vec![route("GET", "/a", "B"), route("GET", "/b", "A"), route("POST", "/c", "B")]);
        list.handle(&ViewEvent::Button(MODE_BUTTON.into()));
        assert_eq!(host.storage().get("routes.mode").map(String::as_str), Some("tree"));
        let view = list.render(base_view());
        let Body::Tree(nodes) = &view.body else { panic!("expected a tree") };
        let shape: Vec<(&str, Option<&str>)> = nodes.iter().map(|n| (n.key.as_str(), n.parent.as_deref())).collect();
        assert_eq!(
            shape,
            [
                ("controller:A", None),
                ("route:1", Some("controller:A")),
                ("controller:B", None),
                ("route:0", Some("controller:B")),
                ("route:2", Some("controller:B")),
            ]
        );
        assert_eq!(view.toolbar.last().unwrap().label, "Flat");
    }

    #[test]
    fn more_than_5000_routes_are_capped_with_a_note() {
        let _host = MockHost::install();
        let routes = (0..5001).map(|i| route("GET", &format!("/r{i}"), "C")).collect();
        let view = list(routes).render(base_view());
        assert_eq!(items(&view).len(), 5000);
        assert_eq!(view.status.as_deref(), Some("showing 5000 of 5001 endpoints"));
    }

    #[test]
    fn activate_opens_the_route() {
        let _host = MockHost::install();
        let mut list = list(vec![route("GET", "/a", "A")]);
        assert_eq!(
            list.handle(&ViewEvent::Activate("route:0".into())),
            RouteEvent::Open(route("GET", "/a", "A"))
        );
        assert_eq!(list.handle(&ViewEvent::Activate("controller:A".into())), RouteEvent::Ignored);
    }

    #[test]
    fn curl_escapes_quotes_in_paths() {
        assert_eq!(curl("get", "http://h:1/", "/a b/{id}"), "curl -X GET 'http://h:1/a b/{id}'");
        assert_eq!(curl("GET", "http://h", "/it's"), "curl -X GET 'http://h/it'\\''s'");
    }

    #[test]
    fn http_request_keeps_path_parameters() {
        assert_eq!(
            http_request("get", "http://h", "/users/{id}"),
            "### GET /users/{id}\nGET http://h/users/{id}\n"
        );
    }

    fn tee_host(status: Option<i32>) -> MockHost {
        let host = MockHost::install().with_capabilities(&["project.read", "process:tee", "clipboard"]);
        host.on_process("tee", move |_, _| Ok(ProcessOutput { status, stdout: String::new(), stderr: String::new() }));
        host
    }

    #[test]
    fn base_prompt_dismissed_writes_and_copies_nothing() {
        let host = tee_host(Some(0));
        let r = route("GET", "/users", "C");
        host.answer_next_prompt(None);
        copy_as_curl(&r, "http://localhost:8080").unwrap();
        host.answer_next_prompt(None);
        create_http_file(&r, "http://localhost:8080", None).unwrap();
        assert_eq!(host.clipboard(), None);
        assert!(host.process_runs().is_empty());
        assert!(host.opened_locations().is_empty());
    }

    #[test]
    fn copy_as_curl_copies_the_command() {
        let host = tee_host(Some(0));
        host.answer_next_prompt(Some(&["http://localhost:9000"]));
        copy_as_curl(&route("GET", "/users", "C"), "http://localhost:8080").unwrap();
        assert_eq!(host.clipboard().as_deref(), Some("curl -X GET 'http://localhost:9000/users'"));
    }

    #[test]
    fn http_file_is_written_with_tee_and_opened() {
        let host = tee_host(Some(0));
        host.answer_next_prompt(Some(&["http://localhost:8080"]));
        create_http_file(&route("GET", "/users/{id}", "C"), "http://localhost:8080", None).unwrap();
        let run = host.process_runs().into_iter().find(|r| r.program == "tee").unwrap();
        assert_eq!(run.args, ["/project/http/get-users-id.http"]);
        assert_eq!(host.opened_locations(), [("/project/http/get-users-id.http".to_owned(), 0, 0)]);
    }

    #[test]
    fn denied_tee_falls_back_to_the_clipboard() {
        let host = MockHost::install().with_capabilities(&["project.read", "clipboard"]);
        host.answer_next_prompt(Some(&["http://localhost:8080"]));
        create_http_file(&route("GET", "/users", "C"), "http://localhost:8080", None).unwrap();
        assert_eq!(
            host.clipboard().as_deref(),
            Some("### GET /users\nGET http://localhost:8080/users\n")
        );
        assert!(host.opened_locations().is_empty());
    }

    #[test]
    fn existing_http_file_is_opened_not_overwritten() {
        let host = tee_host(Some(0));
        host.add_file("http/get-users.http", "old");
        host.answer_next_prompt(Some(&["http://localhost:8080"]));
        create_http_file(&route("GET", "/users", "C"), "http://localhost:8080", None).unwrap();
        assert!(host.process_runs().is_empty());
        assert_eq!(host.opened_locations(), [("/project/http/get-users.http".to_owned(), 0, 0)]);
    }
}
