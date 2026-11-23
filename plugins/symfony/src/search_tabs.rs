//! Search Everywhere tabs (API 1.13): Symfony Routes and Symfony Services, answered from the index
//! the plugin already holds. Nothing is indexed, run or asked from here.
use crate::{Main, under};
use openidea_plugin::search::{SearchItem, SearchPage, SearchProvider, SearchTarget};
use openidea_plugin::{Error, Invocation, Location, Range};

/// Lower is better; `None` when a word of `query` is neither a substring nor a subsequence.
fn score(query: &str, candidate: &str) -> Option<usize> {
    let candidate = candidate.to_lowercase();
    let mut total = 0;
    for word in query.split_whitespace() {
        let word = word.to_lowercase();
        if let Some(at) = candidate.find(&word) {
            total += at;
            continue;
        }
        let mut offset = 0;
        let mut previous = None;
        for needle in word.chars() {
            let next = candidate[offset..].find(needle)? + offset;
            total += 100 + previous.map_or(next, |last| next - last - 1);
            offset = next + needle.len_utf8();
            previous = Some(next);
        }
    }
    Some(total)
}

/// Best matches first (everything for an empty query), at most `limit`.
fn page(candidates: Vec<(String, String, Location)>, query: &str, limit: u32) -> SearchPage {
    let mut ranked: Vec<_> = candidates
        .into_iter()
        .filter_map(|(title, description, location)| Some((score(query, &format!("{title} {description}"))?, title, description, location)))
        .collect();
    ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| (&a.1, &a.2).cmp(&(&b.1, &b.2))));
    ranked.dedup_by(|a, b| (&a.1, &a.2) == (&b.1, &b.2));
    let has_more = ranked.len() > limit as usize;
    let items = ranked
        .into_iter()
        .take(limit as usize)
        .map(|(_, title, description, location)| SearchItem {
            id: format!("{title} {description}"),
            title,
            description: (!description.is_empty()).then_some(description),
            target: SearchTarget::Location(location),
        })
        .collect();
    SearchPage { items, has_more }
}

impl SearchProvider for Main {
    fn search(&mut self, category: &str, query: &str, limit: u32, ctx: &Invocation) -> Result<SearchPage, Error> {
        let (Some(root), Some(index)) = (ctx.project().and_then(|project| project.root().ok()), self.view_index(ctx)) else {
            return Ok(SearchPage::default());
        };
        let candidates: Vec<(String, String, Location)> = match category {
            "routes" => self
                .routes(index)
                .into_iter()
                .filter_map(|route| {
                    let handler = match &route.handler.member {
                        Some(member) => format!("{}::{member}", route.handler.class),
                        None => route.handler.class.clone(),
                    };
                    let description = route.name.into_iter().chain([handler]).collect::<Vec<_>>().join(" · ");
                    Some((format!("{} {}", route.method, route.path), description, route.location?))
                })
                .collect(),
            "services" => index
                .services
                .iter()
                .filter(|(_, entry)| !entry.path.is_empty())
                .map(|(name, entry)| {
                    let location = Location { path: entry.path.clone(), range: Range::new(entry.at, entry.at) };
                    (name.clone(), entry.detail.clone().unwrap_or_default(), location)
                })
                .collect(),
            _ => Vec::new(),
        };
        let owned = candidates.into_iter().filter(|(_, _, location)| under(&location.path, &root)).collect();
        Ok(page(owned, query, limit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::activated;
    use openidea_plugin::mock::MockHost;

    #[test]
    fn routes_tab_finds_routes_by_path_name_or_controller() {
        let host = MockHost::install();
        let mut plugin = activated(&host);

        let page = plugin.search("routes", "blog index", 50, &host.invocation()).unwrap();
        let item = &page.items[0];
        assert_eq!(item.title, "ANY /blog");
        assert_eq!(item.description.as_deref(), Some("blog_index · BlogController::index"));
        assert!(matches!(&item.target, SearchTarget::Location(location) if location.path == "/project/src/Controller/BlogController.php" && location.range.start.line == 4));
        assert!(plugin.search("routes", "nothing-like-this", 50, &host.invocation()).unwrap().items.is_empty());
        assert!(host.process_runs().is_empty());
    }

    #[test]
    fn services_tab_lists_project_services_and_unknown_tabs_are_empty() {
        let host = MockHost::install();
        let mut plugin = activated(&host);

        let page = plugin.search("services", "mailer", 50, &host.invocation()).unwrap();
        assert_eq!((page.items[0].title.as_str(), page.items[0].description.as_deref()), ("app.mailer", Some("App\\Mailer")));
        assert!(page.items.iter().all(|item| matches!(&item.target, SearchTarget::Location(location) if location.path.starts_with("/project/"))));
        assert!(plugin.search("other", "", 50, &host.invocation()).unwrap().items.is_empty());
    }

    #[test]
    fn page_keeps_the_limit_and_reports_more() {
        let location = Location { path: "/project/a.php".into(), range: Range::default() };
        let candidates = vec![("GET /a".to_owned(), String::new(), location.clone()), ("GET /b".to_owned(), String::new(), location.clone()), ("GET /a".to_owned(), String::new(), location)];
        let all = page(candidates.clone(), "", 50);
        assert_eq!(all.items.iter().map(|item| item.title.as_str()).collect::<Vec<_>>(), ["GET /a", "GET /b"], "duplicates are dropped");
        let first = page(candidates, "", 1);
        assert_eq!((first.items.len(), first.has_more), (1, true));
        assert!(score("blog", "ANY /blog").unwrap() < score("bg", "ANY /blog").unwrap());
    }
}
