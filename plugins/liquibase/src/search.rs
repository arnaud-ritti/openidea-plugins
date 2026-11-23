//! The Liquibase Changesets tab of Search Everywhere: changesets by id and author. The
//! index fills a few hundred milliseconds per query, so a large project answers with
//! what is read so far; changed files are read again and the listing is redone after a minute.

use openidea_plugin::search::{SearchItem, SearchPage, SearchProvider, SearchTarget};
use openidea_plugin::{Error, Invocation, Location, Position, Project, Range};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::{Liquibase, MAX_FILES, changelog, root_relative};

const BUDGET: Duration = Duration::from_millis(250);
const RELIST: Duration = Duration::from_secs(60);
const GLOBS: [&str; 5] = ["**/*.xml", "**/*.yaml", "**/*.yml", "**/*.json", "**/*.sql"];

#[derive(Default)]
pub(crate) struct Index {
    root: String,
    listed: Option<Instant>,
    /// Files still to read, last first.
    queue: Vec<String>,
    /// Changesets (id, author, 0-based line) by changelog.
    sets: BTreeMap<String, Vec<(String, String, u32)>>,
}

impl Index {
    /// A changed, created or removed file is read again by the next query.
    pub(crate) fn invalidate(&mut self, path: &str) {
        if self.listed.is_none() || !path.starts_with(&format!("{}/", self.root.trim_end_matches('/'))) {
            return;
        }
        self.sets.remove(path);
        if GLOBS.iter().any(|glob| path.ends_with(glob.trim_start_matches("**/*"))) && !self.queue.iter().any(|queued| queued == path) {
            self.queue.push(path.to_owned());
        }
    }

    fn fill(&mut self, project: &Project, root: &str, deadline: Instant) {
        if self.root != root {
            *self = Index { root: root.to_owned(), ..Index::default() };
        }
        if self.listed.is_none_or(|at| at.elapsed() > RELIST) {
            let mut listed: Vec<String> = GLOBS.iter().flat_map(|glob| project.find_files(glob, MAX_FILES as u32).unwrap_or_default()).collect();
            listed.sort();
            listed.dedup();
            self.sets.retain(|path, _| listed.binary_search(path).is_ok());
            listed.reverse();
            self.queue = listed;
            self.listed = Some(Instant::now());
        }
        while Instant::now() < deadline {
            let Some(path) = self.queue.pop() else { break };
            let small = project.stat(&path).is_ok_and(|metadata| metadata.kind == openidea_plugin::FileKind::File && metadata.size <= 512 * 1024);
            let sets: Vec<_> = match project.read_file(&path).ok().filter(|_| small).and_then(|text| changelog::parse(&path, &text).map(|log| (text, log))) {
                Some((text, log)) => log
                    .change_sets
                    .iter()
                    .filter_map(|set| Some((set.id.clone()?, set.author.clone().unwrap_or_default(), changelog::line_of(&text, set.at))))
                    .collect(),
                None => Vec::new(),
            };
            if sets.is_empty() {
                self.sets.remove(&path);
            } else {
                self.sets.insert(path, sets);
            }
        }
    }
}

impl SearchProvider for Liquibase {
    fn search(&mut self, _category: &str, query: &str, limit: u32, ctx: &Invocation) -> Result<SearchPage, Error> {
        let Some(project) = ctx.project() else { return Ok(SearchPage::default()) };
        let root = project.root()?;
        self.search.fill(&project, &root, Instant::now() + BUDGET);
        let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let limit = limit as usize;
        let mut items = Vec::new();
        let mut matched = 0;
        for (path, sets) in &self.search.sets {
            let relative = root_relative(path, &root);
            for (id, author, line) in sets {
                let haystack = format!("{id} {author} {relative}").to_lowercase();
                if !terms.iter().all(|term| haystack.contains(term.as_str())) {
                    continue;
                }
                matched += 1;
                if items.len() == limit {
                    continue;
                }
                let place = format!("{relative}:{}", line + 1);
                items.push(SearchItem {
                    id: place.clone(),
                    title: id.clone(),
                    description: Some(if author.is_empty() { place } else { format!("{author} · {place}") }),
                    target: SearchTarget::Location(Location { path: path.clone(), range: Range { start: Position::new(*line, 0), end: Position::new(*line, 0) } }),
                });
            }
        }
        Ok(SearchPage { items, has_more: matched > limit || !self.search.queue.is_empty() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    const CHANGELOG: &str = "databaseChangeLog:\n  - changeSet:\n      id: create-users\n      author: alice\n      changes: []\n  - changeSet:\n      id: add-email\n      author: bob\n      changes: []\n";

    fn titles(page: &SearchPage) -> Vec<&str> {
        page.items.iter().map(|item| item.title.as_str()).collect()
    }

    #[test]
    fn finds_changesets_by_id_or_author() {
        let host = MockHost::install();
        host.add_file("db/changelog.yaml", CHANGELOG);
        host.add_file("config/app.yaml", "server:\n  port: 8080\n");
        let mut plugin = Liquibase::default();
        let all = plugin.search("changesets", "", 50, &host.invocation()).unwrap();
        assert_eq!((titles(&all), all.has_more), (vec!["create-users", "add-email"], false));
        let bob = plugin.search("changesets", "BOB", 50, &host.invocation()).unwrap();
        assert_eq!(titles(&bob), ["add-email"]);
        assert_eq!(bob.items[0].description.as_deref(), Some("bob · db/changelog.yaml:6"));
        assert_eq!(
            bob.items[0].target,
            SearchTarget::Location(Location { path: "/project/db/changelog.yaml".into(), range: Range { start: Position::new(5, 0), end: Position::new(5, 0) } })
        );
        let first = plugin.search("changesets", "", 1, &host.invocation()).unwrap();
        assert_eq!((titles(&first), first.has_more), (vec!["create-users"], true));
    }

    #[test]
    fn a_changed_changelog_is_read_again() {
        let host = MockHost::install();
        host.add_file("db/changelog.yaml", CHANGELOG);
        let mut plugin = Liquibase::default();
        plugin.search("changesets", "", 50, &host.invocation()).unwrap();
        host.add_file("db/changelog.yaml", "databaseChangeLog:\n  - changeSet:\n      id: renamed\n      author: alice\n");
        plugin.search.invalidate("/project/db/changelog.yaml");
        assert_eq!(titles(&plugin.search("changesets", "", 50, &host.invocation()).unwrap()), ["renamed"]);
    }

    #[test]
    fn an_exhausted_budget_returns_what_is_read_so_far() {
        let host = MockHost::install();
        host.add_file("db/changelog.yaml", CHANGELOG);
        let project = host.invocation().project().unwrap();
        let mut index = Index::default();
        index.fill(&project, "/project", Instant::now());
        assert!(index.sets.is_empty());
        assert_eq!(index.queue, ["/project/db/changelog.yaml"]);
    }
}
