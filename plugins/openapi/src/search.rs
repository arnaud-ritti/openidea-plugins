//! The OpenAPI Operations tab of Search Everywhere: operations of every spec in the project,
//! matched on method, path, operationId and tag.

use crate::{NO_TAG, file_routes, spec_paths};
use openidea_plugin::search::{SearchItem, SearchPage, SearchTarget};
use openidea_plugin::{Error, Invocation, Location, Project};
use std::time::{Duration, Instant};
use ui_kit::routes::Route;

pub const CATEGORY: &str = "operations";
/// File reading per query: the host gives the whole call 2 s, queueing included.
const SCAN_BUDGET: Duration = Duration::from_millis(300);
/// A complete scan is reused this long, unless a file is saved or changes on disk.
const FRESH: Duration = Duration::from_secs(30);

/// The operations read so far. A scan that runs out of time resumes at the next query.
#[derive(Default)]
pub struct Index {
    root: String,
    paths: Vec<String>,
    next: usize,
    routes: Vec<Route>,
    started: Option<Instant>,
}

impl Index {
    pub fn forget(&mut self) {
        self.started = None;
    }

    fn complete(&self) -> bool {
        self.next >= self.paths.len()
    }

    fn scan(&mut self, project: &Project, budget: Duration) -> Result<(), Error> {
        let root = project.root()?;
        let stale = self.started.is_none_or(|at| self.complete() && at.elapsed() > FRESH);
        if stale || self.root != root {
            *self = Index { paths: spec_paths(project)?, root, started: Some(Instant::now()), ..Index::default() };
        }
        let deadline = Instant::now() + budget;
        while !self.complete() && Instant::now() < deadline {
            self.routes.extend(file_routes(project, &self.paths[self.next]));
            self.next += 1;
        }
        Ok(())
    }

    pub fn search(&mut self, query: &str, limit: u32, ctx: &Invocation) -> Result<SearchPage, Error> {
        self.search_within(query, limit, ctx, SCAN_BUDGET)
    }

    fn search_within(&mut self, query: &str, limit: u32, ctx: &Invocation, budget: Duration) -> Result<SearchPage, Error> {
        let Some(project) = ctx.project() else {
            return Ok(SearchPage::default());
        };
        self.scan(&project, budget)?;
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut found: Vec<(u8, &Route, &Location)> = self
            .routes
            .iter()
            .filter_map(|route| {
                let location = route.location.as_ref()?;
                let id = route.handler.member.as_deref().unwrap_or_default().to_lowercase();
                let text = format!("{} {} {id} {}", route.method, route.path, route.handler.class).to_lowercase();
                if !words.iter().all(|word| text.contains(word.as_str())) {
                    return None;
                }
                let query = query.trim().to_lowercase();
                let rank = match () {
                    _ if !id.is_empty() && id == query => 0,
                    _ if route.path.to_lowercase().starts_with(&query) || (!id.is_empty() && id.starts_with(&query)) => 1,
                    _ => 2,
                };
                Some((rank, route, location))
            })
            .collect();
        found.sort_by_key(|(rank, ..)| *rank);
        let limit = limit as usize;
        let root = format!("{}/", self.root.trim_end_matches('/'));
        let mut ids = std::collections::HashSet::new();
        let items = found
            .iter()
            .map(|(_, route, location)| item(route, location, &root))
            .filter(|item| ids.insert(item.id.clone()))
            .take(limit)
            .collect();
        Ok(SearchPage { items, has_more: found.len() > limit || !self.complete() })
    }
}

fn item(route: &Route, location: &Location, root: &str) -> SearchItem {
    let file = location.path.strip_prefix(root).unwrap_or(&location.path);
    let mut description: Vec<&str> = route.handler.member.iter().map(String::as_str).collect();
    if route.handler.class != NO_TAG {
        description.push(&route.handler.class);
    }
    description.push(file);
    SearchItem {
        id: format!("{}:{}:{}:{} {}", location.path, location.range.start.line, location.range.start.character, route.method, route.path),
        title: format!("{} {}", route.method, route.path),
        description: Some(description.join(" · ")),
        target: SearchTarget::Location(location.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    const PETS: &str = "openapi: 3.1.0\ninfo: {title: Pets, version: '1'}\npaths:\n  /pets:\n    get:\n      operationId: listPets\n      tags: [pets]\n      responses: {200: {description: OK}}\n    post:\n      operationId: createPet\n      responses: {201: {description: Created}}\n  /stores:\n    get:\n      operationId: listStores\n      responses: {200: {description: OK}}\n";

    fn host() -> MockHost {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        host.add_file("api/openapi.yaml", PETS);
        host.add_file("package.json", "{\"name\": \"app\"}");
        host
    }

    #[test]
    fn finds_operations_by_operation_id_method_and_path() {
        let host = host();
        let mut index = Index::default();
        let page = index.search("listpets", 50, &host.invocation()).unwrap();
        assert_eq!(page.items.len(), 1);
        let item = &page.items[0];
        assert_eq!(item.title, "GET /pets");
        assert_eq!(item.description.as_deref(), Some("listPets · pets · api/openapi.yaml"));
        let SearchTarget::Location(location) = &item.target else { panic!() };
        assert_eq!((location.path.as_str(), location.range.start.line), ("/project/api/openapi.yaml", 4));
        assert!(!page.has_more);

        let mut titles = |query: &str| -> Vec<String> {
            index.search(query, 50, &host.invocation()).unwrap().items.into_iter().map(|item| item.title).collect()
        };
        assert_eq!(titles("post"), ["POST /pets"]);
        assert_eq!(titles("get /stores"), ["GET /stores"]);
        assert_eq!(titles("/stores"), ["GET /stores"]);
        assert_eq!(titles(""), ["GET /pets", "POST /pets", "GET /stores"]);
        assert!(titles("nothing").is_empty());
    }

    #[test]
    fn exact_operation_id_ranks_first_and_limit_reports_more() {
        let host = host();
        let mut index = Index::default();
        let page = index.search("list", 1, &host.invocation()).unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(page.has_more);
        assert_eq!(index.search("liststores", 50, &host.invocation()).unwrap().items[0].title, "GET /stores");
    }

    #[test]
    fn a_scan_out_of_time_returns_what_it_has_and_resumes() {
        let host = host();
        let mut index = Index::default();
        let page = index.search_within("", 50, &host.invocation(), Duration::ZERO).unwrap();
        assert!(page.items.is_empty() && page.has_more);
        let page = index.search("", 50, &host.invocation()).unwrap();
        assert_eq!(page.items.len(), 3);
        assert!(!page.has_more);
    }

    #[test]
    fn no_project_gives_no_results() {
        let host = MockHost::install();
        host.set_project_root(None);
        assert_eq!(Index::default().search("pets", 50, &host.invocation()).unwrap(), SearchPage::default());
    }
}
