//! The Flyway Migrations tab of Search Everywhere: local migration files by version and
//! description, from the inventory the checks keep (one file listing when it is missing).

use openidea_plugin::search::{SearchItem, SearchPage, SearchProvider, SearchTarget};
use openidea_plugin::{Error, Invocation, Location, Position, Range};

use crate::config::{load, migration_files};
use crate::naming::{Kind, Name, numeric_key, parse};
use crate::{Flyway, root_relative};

impl SearchProvider for Flyway {
    fn search(&mut self, _category: &str, query: &str, limit: u32, ctx: &Invocation) -> Result<SearchPage, Error> {
        let Some(project) = ctx.project() else { return Ok(SearchPage::default()) };
        let root = project.root()?;
        if !self.local.contains_key(&root) {
            if !self.flyway_project(&project) {
                return Ok(SearchPage::default());
            }
            if !self.local.contains_key(&root) {
                let config = load(&project);
                let files = migration_files(&project, &config);
                self.record_local(&root, &config, &files);
            }
        }
        let Some(local) = self.local.get(&root) else { return Ok(SearchPage::default()) };
        let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut found = Vec::new();
        for path in &local.files {
            let file_name = path.rsplit('/').next().unwrap_or(path);
            let (order, title) = match parse(file_name, &local.naming) {
                Name::Migration { kind, version, description } => {
                    let prefix = match kind {
                        Kind::Versioned => &local.naming.versioned,
                        Kind::Undo => &local.naming.undo,
                        Kind::Baseline => &local.naming.baseline,
                        Kind::Repeatable => &local.naming.repeatable,
                    };
                    let title = format!("{prefix}{version} {}", description.replace('_', " ")).trim_end().to_owned();
                    ((usize::from(kind == Kind::Repeatable), numeric_key(&version)), title)
                }
                Name::Invalid(_) => ((2, Vec::new()), file_name.to_owned()),
                Name::Callback | Name::NotMigration => continue,
            };
            let haystack = format!("{title} {file_name}").to_lowercase();
            if !terms.iter().all(|term| haystack.contains(term.as_str())) {
                continue;
            }
            found.push((order, title, path));
        }
        found.sort();
        let limit = limit as usize;
        let has_more = found.len() > limit;
        let items = found
            .into_iter()
            .take(limit)
            .map(|(_, title, path)| {
                let relative = root_relative(path, &root);
                let description = match local.results.get(path).and_then(|problems| problems.first()) {
                    Some(problem) => format!("{relative} · {}", problem.message),
                    None => relative.to_owned(),
                };
                SearchItem {
                    id: relative.to_owned(),
                    title,
                    description: Some(description),
                    target: SearchTarget::Location(Location { path: path.clone(), range: Range { start: Position::new(0, 0), end: Position::new(0, 0) } }),
                }
            })
            .collect();
        Ok(SearchPage { items, has_more })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    fn titles(page: &SearchPage) -> Vec<&str> {
        page.items.iter().map(|item| item.title.as_str()).collect()
    }

    #[test]
    fn lists_migrations_by_version_and_description() {
        let host = MockHost::install();
        host.add_file("src/main/resources/db/migration/V1__init.sql", "");
        host.add_file("src/main/resources/db/migration/V2__add_users.sql", "");
        host.add_file("src/main/resources/db/migration/R__views.sql", "");
        let mut plugin = Flyway::default();
        let all = plugin.search("migrations", "", 50, &host.invocation()).unwrap();
        assert_eq!(titles(&all), ["V1 init", "V2 add users", "R views"]);
        let users = plugin.search("migrations", "add USERS", 50, &host.invocation()).unwrap();
        assert_eq!(titles(&users), ["V2 add users"]);
        let item = &users.items[0];
        assert_eq!(item.description.as_deref(), Some("src/main/resources/db/migration/V2__add_users.sql"));
        assert_eq!(
            item.target,
            SearchTarget::Location(Location {
                path: "/project/src/main/resources/db/migration/V2__add_users.sql".into(),
                range: Range { start: Position::new(0, 0), end: Position::new(0, 0) },
            })
        );
        let first = plugin.search("migrations", "", 1, &host.invocation()).unwrap();
        assert_eq!((titles(&first), first.has_more), (vec!["V1 init"], true));
    }

    #[test]
    fn a_project_without_flyway_has_no_results() {
        let host = MockHost::install();
        host.add_file("src/main/resources/schema.sql", "");
        let page = Flyway::default().search("migrations", "", 50, &host.invocation()).unwrap();
        assert!(page.items.is_empty());
        assert!(host.console_rules().is_empty());
    }

    #[test]
    fn a_flyway_project_gets_the_console_rules() {
        let host = MockHost::install();
        host.add_file("flyway.toml", "[flyway]\n");
        Flyway::default().search("migrations", "x", 50, &host.invocation()).unwrap();
        assert_eq!(host.console_rules(), crate::console::rules());
    }
}
