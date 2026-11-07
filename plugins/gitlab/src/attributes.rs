//! Editing a merge request's labels, assignees, reviewers, milestone and title with the IDE's pickers.

use openidea_plugin::{Error, PickItem, host};
use serde_json::json;

use crate::api::{MergeRequest, Project, User};

const ATTRIBUTES: [(&str, &str); 5] = [
    ("labels", "Labels"),
    ("assignees", "Assignees"),
    ("reviewers", "Reviewers"),
    ("milestone", "Milestone"),
    ("title", "Title"),
];

/// Asks which attribute to change, then its new value, and applies it. `None` when dismissed.
pub fn edit(api: &Project, mr: &MergeRequest) -> Result<Option<String>, Error> {
    let items: Vec<PickItem> = ATTRIBUTES.iter().map(|(key, label)| PickItem::new(key, label)).collect();
    let Some(attribute) = host::pick(&format!("Edit !{}", mr.iid), &items)? else {
        return Ok(None);
    };

    let (fields, message) = match attribute.as_str() {
        "labels" => {
            let Some(labels) = pick_labels(api, mr)? else { return Ok(None) };
            (json!({ "labels": labels.join(",") }), "Labels updated.")
        }
        "assignees" => {
            let Some(ids) = pick_users(api, "Assignees", &mr.assignees)? else { return Ok(None) };
            (json!({ "assignee_ids": ids }), "Assignees updated.")
        }
        "reviewers" => {
            let Some(ids) = pick_users(api, "Reviewers", &mr.reviewers)? else { return Ok(None) };
            (json!({ "reviewer_ids": ids }), "Reviewers updated.")
        }
        "milestone" => {
            let Some(id) = pick_milestone(api, mr)? else { return Ok(None) };
            (json!({ "milestone_id": id }), "Milestone updated.")
        }
        "title" => {
            let Some(title) = host::input("Title", Some(&mr.title))?.map(|t| t.trim().to_owned()) else {
                return Ok(None);
            };

            if title.is_empty() {
                return Err("A merge request needs a title.".into());
            }
            (json!({ "title": title }), "Title updated.")
        }
        other => return Err(Error::NotFound(format!("attribute {other}"))),
    };

    api.update_merge_request(mr.iid, fields)?;

    Ok(Some(message.to_owned()))
}

fn pick_labels(api: &Project, mr: &MergeRequest) -> Result<Option<Vec<String>>, Error> {
    let items: Vec<PickItem> = api
        .labels()?
        .iter()
        .map(|name| {
            let item = PickItem::new(name, name);
            match mr.labels.contains(name) {
                true => item.description("current"),
                false => item,
            }
        })
        .collect();

    if items.is_empty() {
        return Err("The project has no labels.".into());
    }

    host::pick_many(&format!("Labels (replaces: {})", mr.labels.join(", ")), &items)
}

fn pick_users(api: &Project, title: &str, current: &[User]) -> Result<Option<Vec<u64>>, Error> {
    let items: Vec<PickItem> = api
        .members()?
        .iter()
        .map(|u| {
            let item = PickItem::new(&u.id.to_string(), &u.username).description(&u.name);
            match current.iter().any(|c| c.id == u.id) {
                true => PickItem { description: Some(format!("{} (current)", u.name)), ..item },
                false => item,
            }
        })
        .collect();

    let current_names: Vec<&str> = current.iter().map(|u| u.username.as_str()).collect();
    let picked = host::pick_many(&format!("{title} (replaces: {})", current_names.join(", ")), &items)?;

    Ok(picked.map(|keys| keys.iter().filter_map(|k| k.parse().ok()).collect()))
}

/// `0` clears the milestone.
fn pick_milestone(api: &Project, mr: &MergeRequest) -> Result<Option<u64>, Error> {
    let mut items = vec![PickItem::new("0", "No milestone")];
    items.extend(api.milestones()?.iter().map(|m| PickItem::new(&m.id.to_string(), &m.title)));

    let current = mr.milestone.as_ref().map_or("none", |m| m.title.as_str());

    Ok(host::pick(&format!("Milestone (now: {current})"), &items)?.and_then(|key| key.parse().ok()))
}
