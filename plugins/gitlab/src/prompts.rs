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

pub struct NewSnippet {
    pub title: String,
    pub visibility: String,
    pub extra_paths: Vec<String>,
}

pub struct ShareRequest {
    pub name: String,
    pub visibility: String,
    pub init: bool,
    pub commit_all: bool,
}

fn pick_visibility() -> Result<Option<String>, Error> {
    let items: Vec<PickItem> = api::VISIBILITIES.iter().map(|v| PickItem::new(v, v)).collect();

    host::pick("Visibility", &items)
}

/// An empty title means `default_title`; `paths` are the files already in the snippet and are not asked twice.
pub fn ask_new_snippet(default_title: &str, paths: &[String]) -> Result<Option<NewSnippet>, Error> {
    let Some(typed) = host::input("Snippet title", Some(default_title))? else {
        return Ok(None);
    };
    let title = match typed.trim() {
        "" => default_title.to_owned(),
        typed => typed.to_owned(),
    };
    let Some(visibility) = pick_visibility()? else {
        return Ok(None);
    };
    if visibility == "public" && !host::confirm("Create Snippet", "Publish publicly? Anyone can see a public snippet, signed in or not.")? {
        return Ok(None);
    }

    let mut extra_paths: Vec<String> = Vec::new();
    loop {
        let Some(path) = host::input("Add another project file (path from the project root; empty to finish)", None)? else {
            return Ok(None);
        };
        let path = path.trim();
        if path.is_empty() {
            break;
        }
        if !extra_paths.iter().any(|p| p == path) && !paths.iter().any(|p| p == path) {
            extra_paths.push(path.to_owned());
        }
    }

    Ok(Some(NewSnippet { title, visibility, extra_paths }))
}

/// An empty name means `default_name`.
pub fn ask_share(default_name: &str) -> Result<Option<ShareRequest>, Error> {
    let Some(typed) = host::input("Project name", Some(default_name))? else {
        return Ok(None);
    };
    let name = match typed.trim() {
        "" => default_name.to_owned(),
        typed => typed.to_owned(),
    };
    let Some(visibility) = pick_visibility()? else {
        return Ok(None);
    };
    if visibility == "public" && !host::confirm("Share Project", "Make the project public? Anyone can see it, signed in or not.")? {
        return Ok(None);
    }

    Ok(Some(ShareRequest { name, visibility, init: false, commit_all: false }))
}
