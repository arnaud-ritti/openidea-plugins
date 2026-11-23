//! The Spring Endpoints tab of Search Everywhere: every indexed HTTP route,
//! controller mappings included (the IDE's own Endpoints section only gets the
//! functional routes, see `WebTools::ide_routes`).

use super::Endpoints;
use openidea_plugin::search::{SearchItem, SearchPage, SearchProvider, SearchTarget};
use openidea_plugin::{Error, Invocation};
use spring_text::search::{SEARCH_BUDGET, page};

pub(crate) const CATEGORY: &str = "routes";

impl SearchProvider for Endpoints {
    fn search(&mut self, category: &str, query: &str, limit: u32, ctx: &Invocation) -> Result<SearchPage, Error> {
        let Some(project) = ctx.project().filter(|_| category == CATEGORY) else {
            return Ok(SearchPage::default());
        };
        let found = self.web.routes(&project, SEARCH_BUDGET);
        let items = found.routes.into_iter().filter_map(|route| {
            let location = route.location?;
            let handler = match &route.handler.member {
                Some(member) => format!("{}.{member}", route.handler.class),
                None => route.handler.class.clone(),
            };
            let description = match route.name {
                Some(name) => format!("{handler} ({name})"),
                None => handler,
            };
            Some(SearchItem {
                id: format!("{} {} {}:{}:{}", route.method, route.path, location.path, location.range.start.line, location.range.start.character),
                title: format!("{} {}", route.method, route.path),
                description: Some(description).filter(|text| !text.is_empty()),
                target: SearchTarget::Location(location),
            })
        });
        Ok(page(items, query, limit, found.scanning))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use std::time::{Duration, Instant};

    const USERS: &str = "package com.acme;\n\n@RestController\n@RequestMapping(\"/api\")\nclass Users {\n    @GetMapping(\"/users/{id}\")\n    User show() { return null; }\n\n    @PostMapping(\"/users\")\n    User create() { return null; }\n}\n";
    const ORDERS: &str = "package com.acme;\n\n@RestController\nclass Orders {\n    @GetMapping(\"/orders\")\n    List<Order> list() { return null; }\n}\n";

    #[test]
    fn spring_endpoints_tab_finds_controller_mappings_and_opens_their_declaration() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Users.java", USERS);
        host.add_file("src/main/java/com/acme/Orders.java", ORDERS);
        let found = Endpoints::default().search(CATEGORY, "get users", 50, &host.invocation()).unwrap();
        assert_eq!(found.items.len(), 1, "{found:?}");
        let item = &found.items[0];
        assert_eq!(item.title, "GET /api/users/{id}");
        assert_eq!(item.description.as_deref(), Some("Users.show"));
        let SearchTarget::Location(location) = &item.target else { panic!("location expected") };
        assert_eq!(location.path, "/project/src/main/java/com/acme/Users.java");
        assert_eq!(location.range.start.line, 6);
        assert!(!found.has_more);

        let all = Endpoints::default().search(CATEGORY, "", 50, &host.invocation()).unwrap();
        assert_eq!(all.items.len(), 3);
    }

    #[test]
    fn other_categories_and_no_project_return_nothing() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Orders.java", ORDERS);
        assert!(Endpoints::default().search("beans", "", 50, &host.invocation()).unwrap().items.is_empty());
        host.set_project_root(None);
        assert!(Endpoints::default().search(CATEGORY, "", 50, &host.invocation()).unwrap().items.is_empty());
    }

    #[test]
    fn a_large_cold_project_answers_well_within_the_host_deadline_and_offers_more() {
        let host = MockHost::install();
        for n in 0..3000 {
            host.add_file(&format!("src/main/java/com/acme/m{n}/Orders{n}.java"), &ORDERS.replace("Orders", &format!("Orders{n}")));
        }
        let started = Instant::now();
        let found = Endpoints::default().search(CATEGORY, "orders", 50, &host.invocation()).unwrap();
        assert!(started.elapsed() < Duration::from_millis(1500), "{:?}", started.elapsed());
        assert!(found.has_more);
    }
}
