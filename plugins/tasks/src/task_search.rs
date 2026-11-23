//! The Tasks tab of Search Everywhere: open tasks by id, summary or branch; choosing one
//! switches to it.

use openidea_plugin::search::{SearchItem, SearchPage, SearchProvider, SearchTarget};
use openidea_plugin::{Error, Invocation};

use crate::{Tasks, load};

impl SearchProvider for Tasks {
    fn search(&mut self, _category: &str, query: &str, limit: u32, ctx: &Invocation) -> Result<SearchPage, Error> {
        let Some(root) = ctx.project().and_then(|project| project.root().ok()) else { return Ok(SearchPage::default()) };
        let tasks = load(&root)?;
        let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let found: Vec<_> = tasks
            .tasks
            .iter()
            .filter(|task| !task.closed)
            .filter(|task| {
                let haystack = format!("{} {} {}", task.id, task.summary, task.branch.as_deref().unwrap_or_default()).to_lowercase();
                terms.iter().all(|term| haystack.contains(term.as_str()))
            })
            .collect();
        let items = found
            .iter()
            .take(limit as usize)
            .map(|task| {
                let state = if task.id == tasks.active { "Active" } else { "Switch to this task" };
                SearchItem {
                    id: task.id.clone(),
                    title: task.label(),
                    description: Some(match &task.branch {
                        Some(branch) => format!("{state} · {branch}"),
                        None => state.to_owned(),
                    }),
                    target: SearchTarget::Command { id: "switch".into(), arg: Some(task.id.clone()) },
                }
            })
            .collect();
        Ok(SearchPage { items, has_more: found.len() > limit as usize })
    }
}
