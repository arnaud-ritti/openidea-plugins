use openidea_plugin::host::storage;
use openidea_plugin::views::{Button, View};

pub const SECTION_KEY: &str = "gitlab.section";
const BUTTON_PREFIX: &str = "section:";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Overview,
    Changes,
    Commits,
    Files,
    Pipelines,
}

impl Section {
    pub const ALL: [Section; 5] = [Section::Overview, Section::Changes, Section::Commits, Section::Files, Section::Pipelines];

    pub fn id(self) -> &'static str {
        match self {
            Section::Overview => "overview",
            Section::Changes => "changes",
            Section::Commits => "commits",
            Section::Files => "files",
            Section::Pipelines => "pipelines",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Section::Overview => "Overview",
            Section::Changes => "Changes",
            Section::Commits => "Commits",
            Section::Files => "Files",
            Section::Pipelines => "Pipelines",
        }
    }

    pub fn parse(id: &str) -> Option<Section> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    pub fn from_button(button: &str) -> Option<Section> {
        Self::parse(button.strip_prefix(BUTTON_PREFIX)?)
    }
}

impl Section {
    /// The stored section, Overview when nothing or something unknown is stored.
    pub fn stored() -> Section {
        let stored = storage::get(SECTION_KEY).ok().flatten();

        stored.as_deref().and_then(Section::parse).unwrap_or(Section::Overview)
    }
}

#[derive(Default)]
pub struct Badges {
    pub changes: Option<usize>,
    pub commits: Option<usize>,
    pub files: Option<usize>,
    pub pipeline: Option<String>,
}

fn button_label(section: Section, active: bool, badges: &Badges) -> String {
    let badge = match section {
        Section::Overview => None,
        Section::Changes => badges.changes.map(|n| n.to_string()),
        Section::Commits => badges.commits.map(|n| n.to_string()),
        Section::Files => badges.files.map(|n| n.to_string()),
        Section::Pipelines => badges.pipeline.clone(),
    };
    let label = match badge {
        Some(badge) => format!("{} ({badge})", section.label()),
        None => section.label().to_owned(),
    };

    if active { format!("● {label}") } else { label }
}

pub fn with_sections(mut view: View, active: Section, badges: &Badges) -> View {
    let mut toolbar: Vec<Button> = Section::ALL
        .into_iter()
        .map(|s| Button::new(&format!("{BUTTON_PREFIX}{}", s.id()), &button_label(s, s == active, badges)))
        .collect();
    toolbar.append(&mut view.toolbar);
    view.toolbar = toolbar;

    view
}
