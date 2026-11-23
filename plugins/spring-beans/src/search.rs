//! The Spring Beans tab of Search Everywhere: every bean the index holds, by
//! name, with its kind, type and file, opening its declaration.

use crate::Navigator;
use openidea_plugin::search::{SearchItem, SearchPage, SearchProvider, SearchTarget};
use openidea_plugin::{Error, Invocation, Location};
use spring_text::search::{SEARCH_BUDGET, page};

pub(crate) const CATEGORY: &str = "beans";

impl SearchProvider for Navigator {
    fn search(&mut self, category: &str, query: &str, limit: u32, ctx: &Invocation) -> Result<SearchPage, Error> {
        let Some(project) = ctx.project().filter(|_| category == CATEGORY) else {
            return Ok(SearchPage::default());
        };
        let cache = self.indexes.get(&project, SEARCH_BUDGET);
        let items = cache.files().iter().flat_map(|(path, model)| {
            let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
            model.beans.iter().map(move |bean| {
                let description = [bean.kind.as_str(), bean.ty.as_str(), file].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" · ");
                SearchItem {
                    id: format!("{path}:{}:{}:{}", bean.range.start.line, bean.range.start.character, bean.name),
                    title: bean.name.clone(),
                    description: Some(description),
                    target: SearchTarget::Location(Location { path: path.clone(), range: bean.range }),
                }
            })
        });
        Ok(page(items, query, limit, cache.scanning()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    const SERVICE: &str = "package com.acme;\n\nimport org.springframework.stereotype.Service;\n\n@Service\npublic class UserService {}\n";
    const CONFIG: &str = "package com.acme;\n\nimport org.springframework.context.annotation.Bean;\nimport org.springframework.context.annotation.Configuration;\n\n@Configuration\npublic class AppConfig {\n    @Bean\n    public Clock clock() { return Clock.systemUTC(); }\n}\n";

    #[test]
    fn spring_beans_tab_finds_beans_by_name_kind_or_type_and_opens_their_declaration() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/UserService.java", SERVICE);
        host.add_file("src/main/java/com/acme/AppConfig.java", CONFIG);
        let found = Navigator::default().search(CATEGORY, "clock", 50, &host.invocation()).unwrap();
        assert_eq!(found.items.len(), 1, "{found:?}");
        let item = &found.items[0];
        assert_eq!(item.title, "clock");
        assert_eq!(item.description.as_deref(), Some("@Bean · Clock · AppConfig.java"));
        let SearchTarget::Location(location) = &item.target else { panic!("location expected") };
        assert_eq!(location.path, "/project/src/main/java/com/acme/AppConfig.java");
        assert_eq!(location.range.start.line, 8);

        let services = Navigator::default().search(CATEGORY, "@service", 50, &host.invocation()).unwrap();
        assert_eq!(services.items.iter().map(|item| item.title.as_str()).collect::<Vec<_>>(), ["userService"]);
    }

    #[test]
    fn other_categories_and_no_project_return_nothing() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/UserService.java", SERVICE);
        assert!(Navigator::default().search("routes", "", 50, &host.invocation()).unwrap().items.is_empty());
        host.set_project_root(None);
        assert!(Navigator::default().search(CATEGORY, "", 50, &host.invocation()).unwrap().items.is_empty());
    }
}
