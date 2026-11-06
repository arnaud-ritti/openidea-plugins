//! Prompt sequences that replace the form windows: each asks in order and returns `None` when any is dismissed.

use openidea_plugin::host;
use openidea_plugin::{Error, PickItem};

use crate::{api, safe};

/// The title prefilled, and the branches to choose from (each list starts with its default).
pub struct MrDefaults {
    pub title: String,
    pub sources: Vec<String>,
    pub targets: Vec<String>,
}

pub struct NewMergeRequest {
    pub source: String,
    pub target: String,
    pub title: String,
    pub description: String,
    pub draft: bool,
}

impl NewMergeRequest {
    pub fn into_request(self) -> Result<api::NewMergeRequest, Error> {
        let title = self.title.trim();
        if title.is_empty() {
            return Err("Write a title first.".into());
        }

        let title = match self.draft && !title.to_ascii_lowercase().starts_with("draft:") {
            true => format!("Draft: {title}"),
            false => title.to_owned(),
        };

        Ok(api::NewMergeRequest {
            source_branch: safe::ref_name(&self.source)?.to_owned(),
            target_branch: safe::ref_name(&self.target)?.to_owned(),
            title,
            description: self.description,
            ..Default::default()
        })
    }
}

fn pick_branch(title: &str, branches: &[String]) -> Result<Option<String>, Error> {
    let items: Vec<PickItem> = branches.iter().map(|b| PickItem::new(b, b)).collect();

    host::pick(title, &items)
}

pub fn ask_new_merge_request(defaults: &MrDefaults) -> Result<Option<NewMergeRequest>, Error> {
    let Some(source) = pick_branch("Source branch", &defaults.sources)? else {
        return Ok(None);
    };
    let Some(target) = pick_branch("Target branch", &defaults.targets)? else {
        return Ok(None);
    };
    let Some(title) = host::input("Merge request title", Some(&defaults.title))? else {
        return Ok(None);
    };
    let Some(description) = host::input("Description (one line; use Open on GitLab for more)", None)? else {
        return Ok(None);
    };
    let draft = host::confirm("Create Merge Request", "Create as draft?")?;

    Ok(Some(NewMergeRequest { source, target, title, description, draft }))
}
