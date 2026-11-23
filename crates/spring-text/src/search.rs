//! Search Everywhere tabs (API 1.13): query matching shared by the Spring plugins.

use openidea_plugin::search::{SearchItem, SearchPage};
use std::collections::BTreeSet;
use std::time::Duration;

/// Index reading time per search call. The host allows 2 s including queueing,
/// so a search answers from what is already indexed plus this short scan.
pub const SEARCH_BUDGET: Duration = Duration::from_millis(300);

/// The first `limit` items holding every word of `query` (case-insensitive, in the
/// title or description), titles starting with or containing the whole query first.
/// Items the host would refuse (duplicate id, control characters, oversized text) are
/// dropped so one of them can't reject the page. `incomplete` (the index is still
/// being read) offers Show more, which searches again with a fresh budget.
pub fn page(items: impl IntoIterator<Item = SearchItem>, query: &str, limit: u32, incomplete: bool) -> SearchPage {
    let query = query.trim().to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut ids = BTreeSet::new();
    let mut ranked: Vec<(u8, SearchItem)> = items
        .into_iter()
        .filter(|item| valid(item) && ids.insert(item.id.clone()))
        .filter_map(|item| {
            let title = item.title.to_lowercase();
            let text = format!("{title} {}", item.description.as_deref().unwrap_or_default().to_lowercase());
            if !words.iter().all(|word| text.contains(word)) {
                return None;
            }
            let rank = if title.starts_with(&query) {
                0
            } else if title.contains(&query) {
                1
            } else {
                2
            };
            Some((rank, item))
        })
        .collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    let has_more = incomplete || ranked.len() > limit as usize;
    SearchPage { items: ranked.into_iter().take(limit as usize).map(|(_, item)| item).collect(), has_more }
}

fn valid(item: &SearchItem) -> bool {
    let clean = |text: &str| !text.chars().any(char::is_control);
    !item.title.is_empty()
        && item.title.len() <= 1024
        && clean(&item.title)
        && item.description.as_deref().is_none_or(|text| text.len() <= 4096 && clean(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::search::SearchTarget;

    fn item(id: &str, title: &str, description: &str) -> SearchItem {
        SearchItem {
            id: id.into(),
            title: title.into(),
            description: Some(description.into()),
            target: SearchTarget::Command { id: "open".into(), arg: None },
        }
    }

    fn titles(page: &SearchPage) -> Vec<&str> {
        page.items.iter().map(|item| item.title.as_str()).collect()
    }

    #[test]
    fn every_word_must_match_and_title_prefix_ranks_first() {
        let items = vec![
            item("1", "GET /orders/{id}", "OrderController.show"),
            item("2", "POST /users", "UserController.create"),
            item("3", "GET /users/{id}", "UserController.show"),
        ];
        let found = page(items.clone(), "get users", 50, false);
        assert_eq!(titles(&found), ["GET /users/{id}"]);
        let found = page(items.clone(), "/USERS", 50, false);
        assert_eq!(titles(&found), ["POST /users", "GET /users/{id}"]);
        let found = page(items, "usercontroller", 50, false);
        assert_eq!(titles(&found), ["POST /users", "GET /users/{id}"]);
    }

    #[test]
    fn empty_query_lists_everything_up_to_the_limit() {
        let items = (0..5).map(|n| item(&n.to_string(), &format!("bean{n}"), "@Service"));
        let found = page(items, "", 3, false);
        assert_eq!(found.items.len(), 3);
        assert!(found.has_more);
    }

    #[test]
    fn items_the_host_would_refuse_are_dropped() {
        let items = vec![
            item("1", "ok", ""),
            item("1", "duplicate id", ""),
            item("2", "tab\there", ""),
            item("3", "", ""),
            item("4", &"x".repeat(1025), ""),
        ];
        let found = page(items, "", 50, false);
        assert_eq!(titles(&found), ["ok"]);
        assert!(!found.has_more);
    }

    #[test]
    fn an_incomplete_index_offers_more() {
        assert!(page(vec![item("1", "a", "")], "", 50, true).has_more);
    }
}
