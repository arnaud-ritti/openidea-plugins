//! Form views that create things on GitLab: the fields as typed, and the request they become.

use openidea_plugin::Error;
use openidea_plugin::views::{Body, Button, Field, FieldValue, View};

use crate::api::{self, Namespace, SnippetDraft};
use crate::safe;

pub const PERSONAL: &str = "Personal";

/// The fields of the Create Snippet view; the content is the focused file or its selection.
pub struct SnippetForm {
    pub title: String,
    pub description: String,
    pub visibility: String,
    /// `Personal` or `Project <group/name>`, one of the listed scopes.
    pub scope: String,
    pub file_path: String,
    /// Project files, one repository path per line, added to the snippet beside the focused file.
    pub more_files: String,
    /// The `(path, content)` a public create warned about; only `confirm-public` posts it, and only unchanged.
    pub confirm_public: Option<(String, String)>,
    pub status: Option<String>,
}

impl Default for SnippetForm {
    fn default() -> Self {
        Self {
            title: String::new(),
            description: String::new(),
            visibility: "private".into(),
            scope: PERSONAL.into(),
            file_path: String::new(),
            more_files: String::new(),
            confirm_public: None,
            status: None,
        }
    }
}

impl SnippetForm {
    pub fn render(&self, scopes: &[String]) -> View {
        let visibilities = api::VISIBILITIES.map(str::to_owned).to_vec();
        let fields = vec![
            Field::new("title", "Title (empty: the file name)", FieldValue::Text(self.title.clone())),
            Field::new("description", "Description", FieldValue::Multiline(self.description.clone())),
            Field::new("visibility", "Visibility", FieldValue::Choice(visibilities, self.visibility.clone())),
            Field::new("scope", "Snippet of", FieldValue::Choice(scopes.to_vec(), self.scope.clone())),
            Field::new("file-path", "File name (empty: the focused file's path)", FieldValue::Text(self.file_path.clone())),
            Field::new("more-files", "More files (paths from the project root, one per line)", FieldValue::Multiline(self.more_files.clone())),
        ];

        let create = match self.confirm_public.is_some() {
            true => Button::new("confirm-public", "Confirm public").icon("warning"),
            false => Button::new("create", "Create Snippet").icon("plus"),
        };
        let view = View::new(Body::Form(fields)).button(create);

        match &self.status {
            Some(status) => view.status(status),
            None => view,
        }
    }

    /// The caller checks a `scope` value against the listed scopes first.
    pub fn field(&mut self, key: &str, value: &str) {
        match key {
            "title" => self.title = value.to_owned(),
            "description" => self.description = value.to_owned(),
            "visibility" if api::VISIBILITIES.contains(&value) => self.visibility = value.to_owned(),
            "scope" => self.scope = value.to_owned(),
            "file-path" => self.file_path = value.to_owned(),
            "more-files" => self.more_files = value.to_owned(),
            _ => return,
        }

        self.confirm_public = None;
    }

    pub fn extra_paths(&self) -> Result<Vec<&str>, Error> {
        self.more_files.lines().map(str::trim).filter(|l| !l.is_empty()).map(safe::repo_file_path).collect()
    }

    /// The first file is the focused one: its path is its repository path, or its name outside the repository.
    pub fn draft(&self, mut files: Vec<(String, String)>, scopes: &[String]) -> Result<SnippetDraft, Error> {
        if !scopes.contains(&self.scope) {
            return Err("Choose what the snippet belongs to.".into());
        }

        let focused = files.first_mut().ok_or("Nothing to put in the snippet.")?;
        if !self.file_path.trim().is_empty() {
            focused.0 = safe::repo_file_path(self.file_path.trim())?.to_owned();
        }
        let file_path = focused.0.clone();
        let title = match self.title.trim() {
            "" => file_path.rsplit('/').next().unwrap_or(&file_path).to_owned(),
            typed => typed.to_owned(),
        };

        Ok(SnippetDraft {
            title,
            description: self.description.clone(),
            visibility: self.visibility.clone(),
            files,
            project: self.scope.strip_prefix("Project ").map(str::to_owned),
        })
    }
}

/// The fields of the Share Project view; empty name and remote mean the folder name and `origin` (else `gitlab`).
pub struct ShareForm {
    pub name: String,
    /// `Personal` or the full path of a listed namespace.
    pub namespace: String,
    pub description: String,
    pub remote: String,
    pub visibility: String,
    pub init: bool,
    pub commit_all: bool,
    /// The files a commit-all click listed; only `confirm-commit` commits, and only while git lists exactly these.
    pub confirm_commit: Option<Vec<String>>,
    pub status: Option<String>,
}

impl Default for ShareForm {
    fn default() -> Self {
        Self {
            name: String::new(),
            namespace: PERSONAL.into(),
            description: String::new(),
            remote: String::new(),
            visibility: "private".into(),
            init: false,
            commit_all: false,
            confirm_commit: None,
            status: None,
        }
    }
}

/// `Personal`, then the namespaces the user can create projects in.
pub fn namespace_choices(namespaces: &[Namespace]) -> Vec<String> {
    std::iter::once(PERSONAL.to_owned()).chain(namespaces.iter().map(|n| n.full_path.clone())).collect()
}

impl ShareForm {
    pub fn render(&self, namespaces: &[Namespace]) -> View {
        let text = |key: &str, label: &str, value: &str| Field::new(key, label, FieldValue::Text(value.to_owned()));
        let mut fields = vec![
            text("name", "Name (empty: the folder name)", &self.name),
            Field::new("namespace", "Namespace", FieldValue::Choice(namespace_choices(namespaces), self.namespace.clone())),
            Field::new("description", "Description", FieldValue::Multiline(self.description.clone())),
            text("remote", "Remote name (empty: origin, or gitlab when origin exists)", &self.remote),
            Field::new("visibility", "Visibility", FieldValue::Choice(api::VISIBILITIES.map(str::to_owned).to_vec(), self.visibility.clone())),
            Field::new("init", "Create a git repository when there is none", FieldValue::Checkbox(self.init)),
            Field::new("commit-all", "Commit all files first (lists them for a confirm)", FieldValue::Checkbox(self.commit_all)),
        ];

        let share = match &self.confirm_commit {
            Some(files) => {
                fields.push(Field::new("files", "Files to commit (read-only)", FieldValue::Multiline(files.join("\n"))));
                Button::new("confirm-commit", "Confirm commit").icon("warning")
            }
            None => Button::new("share", "Share Project").icon("plus"),
        };
        let view = View::new(Body::Form(fields)).button(share).button(Button::new("refresh", "Refresh").icon("refresh"));

        match &self.status {
            Some(status) => view.status(status),
            None => view,
        }
    }

    /// The caller checks a `namespace` value against [`namespace_choices`] first. Any edit cancels a pending confirm.
    pub fn field(&mut self, key: &str, value: &str) {
        let checked = value == "true";

        match key {
            "name" => self.name = value.to_owned(),
            "namespace" => self.namespace = value.to_owned(),
            "description" => self.description = value.to_owned(),
            "remote" => self.remote = value.to_owned(),
            "visibility" if api::VISIBILITIES.contains(&value) => self.visibility = value.to_owned(),
            "init" => self.init = checked,
            "commit-all" => self.commit_all = checked,
            _ => return,
        }

        self.confirm_commit = None;
    }

    pub fn namespace_id(&self, namespaces: &[Namespace]) -> Result<Option<u64>, Error> {
        if self.namespace == PERSONAL {
            return Ok(None);
        }

        let namespace = namespaces.iter().find(|n| n.full_path == self.namespace).ok_or("Choose a namespace.")?;

        Ok(Some(namespace.id))
    }
}
