//! Form views that create things on GitLab: the fields as typed, and the request they become.

use openidea_plugin::Error;
use openidea_plugin::views::{Body, Button, Field, FieldValue, View};

use crate::api::{self, NewMergeRequest};
use crate::safe;

/// The fields of the Create Merge Request view. The source branch is the checked-out one, never a field.
#[derive(Default)]
pub struct MergeRequestForm {
    pub title: String,
    pub description: String,
    pub target: String,
    pub assignee: String,
    pub reviewers: String,
    pub labels: String,
    pub draft: bool,
    pub squash: bool,
    pub remove_source: bool,
}

/// `ann, @bob carl` as `["ann", "bob", "carl"]`.
fn usernames(text: &str) -> Vec<&str> {
    text.split([',', ' ', '\t', '\n']).map(|name| name.trim().trim_start_matches('@')).filter(|name| !name.is_empty()).collect()
}

impl MergeRequestForm {
    pub fn render(&self, branches: &[String], source: &str) -> View {
        let text = |key: &str, label: &str, value: &str| Field::new(key, label, FieldValue::Text(value.to_owned()));
        let fields = vec![
            text("title", "Title", &self.title),
            Field::new("description", "Description", FieldValue::Multiline(self.description.clone())),
            Field::new("target-branch", "Target branch", FieldValue::Choice(branches.to_vec(), self.target.clone())),
            text("assignee", "Assignee (username)", &self.assignee),
            text("reviewers", "Reviewers (usernames, comma-separated)", &self.reviewers),
            text("labels", "Labels (comma-separated)", &self.labels),
            Field::new("draft", "Mark as draft", FieldValue::Checkbox(self.draft)),
            Field::new("squash", "Squash commits when merging", FieldValue::Checkbox(self.squash)),
            Field::new("remove-source-branch", "Delete source branch when merging", FieldValue::Checkbox(self.remove_source)),
        ];

        View::new(Body::Form(fields))
            .button(Button::new("create", "Create Merge Request").icon("github-pr-open"))
            .button(Button::new("refresh", "Refresh").icon("refresh"))
            .status(&format!("{source} → {}", self.target))
    }

    /// The caller checks a `target-branch` value against the listed branches first.
    pub fn field(&mut self, key: &str, value: &str) {
        let checked = value == "true";

        match key {
            "title" => self.title = value.to_owned(),
            "description" => self.description = value.to_owned(),
            "target-branch" => self.target = value.to_owned(),
            "assignee" => self.assignee = value.to_owned(),
            "reviewers" => self.reviewers = value.to_owned(),
            "labels" => self.labels = value.to_owned(),
            "draft" => self.draft = checked,
            "squash" => self.squash = checked,
            "remove-source-branch" => self.remove_source = checked,
            _ => {}
        }
    }

    /// Every username is checked before the first lookup, so a bad one sends nothing.
    pub fn request(&self, source: &str, branches: &[String]) -> Result<NewMergeRequest, Error> {
        let title = self.title.trim();
        if title.is_empty() {
            return Err("Write a title first.".into());
        }

        if !branches.contains(&self.target) {
            return Err("Choose a target branch.".into());
        }

        let assignee = usernames(&self.assignee);
        if assignee.len() > 1 {
            return Err("Enter one assignee.".into());
        }

        let reviewers = usernames(&self.reviewers);
        for name in assignee.iter().chain(&reviewers) {
            safe::username(name)?;
        }

        let title = match self.draft && !title.to_ascii_lowercase().starts_with("draft:") {
            true => format!("Draft: {title}"),
            false => title.to_owned(),
        };

        Ok(NewMergeRequest {
            source_branch: safe::ref_name(source)?.to_owned(),
            target_branch: self.target.clone(),
            title,
            description: self.description.clone(),
            assignee_id: assignee.first().map(|name| api::user_id(name)).transpose()?,
            reviewer_ids: reviewers.into_iter().map(api::user_id).collect::<Result<_, _>>()?,
            labels: self.labels.split(',').map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect(),
            squash: self.squash,
            remove_source_branch: self.remove_source,
        })
    }
}
