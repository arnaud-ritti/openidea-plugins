//! The Dependencies section: the server's catalogue as a tree by category,
//! with the chosen ids kept in the form's `dependencies` field.

use crate::{Launcher, START, starters};
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent};
use openidea_plugin::{Error, host};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

const SELECTED: &str = "selected";
const CATEGORY_PREFIX: &str = "cat:";
const DEPENDENCY_PREFIX: &str = "dep:";
const CHOSEN_PREFIX: &str = "sel:";

pub struct Dependency {
    pub id: String,
    pub name: String,
    pub description: String,
    pub range: Option<String>,
}

pub struct Category {
    pub name: String,
    pub dependencies: Vec<Dependency>,
}

pub struct Catalogue {
    pub default_boot: String,
    pub categories: Vec<Category>,
}

impl Catalogue {
    pub fn load() -> Result<Catalogue, Error> {
        let metadata = starters::get("/metadata/client")?;
        let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
        let categories = metadata["dependencies"]["values"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|group| Category {
                name: text(&group["name"]),
                dependencies: group["values"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|d| {
                        let id = d["id"].as_str()?;
                        Some(Dependency {
                            id: id.to_owned(),
                            name: d["name"].as_str().unwrap_or(id).to_owned(),
                            description: text(&d["description"]),
                            range: d["compatibilityRange"].as_str().map(str::to_owned),
                        })
                    })
                    .collect(),
            })
            .collect();

        Ok(Catalogue { default_boot: text(&metadata["bootVersion"]["default"]), categories })
    }

    fn find(&self, id: &str) -> Option<&Dependency> {
        self.categories.iter().flat_map(|c| &c.dependencies).find(|d| d.id == id)
    }
}

#[derive(Default)]
pub struct State {
    catalogue: Option<Result<Catalogue, String>>,
    search: Option<String>,
    expansion: BTreeMap<String, bool>,
}

fn version_parts(version: &str) -> Option<(Vec<u64>, (u8, u64))> {
    let (numbers, qualifier) = version.split_once('-').unwrap_or((version, ""));
    let parts: Vec<u64> = numbers.split('.').map_while(|p| p.parse().ok()).collect();
    if parts.is_empty() {
        return None;
    }

    let digits_at = qualifier.find(|c: char| c.is_ascii_digit()).unwrap_or(qualifier.len());
    let number = qualifier[digits_at..].parse().unwrap_or(0);
    let rank = match qualifier[..digits_at].to_ascii_uppercase().as_str() {
        "M" => 0,
        "RC" => 1,
        "SNAPSHOT" => 2,
        _ => 3,
    };

    Some((parts, (rank, number)))
}

pub fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    let (mut a_parts, a_qualifier) = version_parts(a)?;
    let (mut b_parts, b_qualifier) = version_parts(b)?;
    let length = a_parts.len().max(b_parts.len());
    a_parts.resize(length, 0);
    b_parts.resize(length, 0);

    Some(a_parts.cmp(&b_parts).then(a_qualifier.cmp(&b_qualifier)))
}

/// Whether `version` is inside `range`; true when either cannot be parsed.
pub fn in_range(version: &str, range: &str) -> bool {
    let range = range.trim();
    let bounded = range.starts_with(['[', '(']) && range.ends_with([']', ')']);
    let (low, low_inclusive, high, high_inclusive) = if bounded {
        let Some((low, high)) = range[1..range.len() - 1].split_once(',') else { return true };
        (low.trim(), range.starts_with('['), high.trim(), range.ends_with(']'))
    } else {
        (range, true, "", false)
    };

    let above_low = low.is_empty() || match compare_versions(version, low) {
        Some(Ordering::Greater) => true,
        Some(Ordering::Equal) => low_inclusive,
        Some(Ordering::Less) => false,
        None => true,
    };
    let below_high = high.is_empty() || match compare_versions(version, high) {
        Some(Ordering::Less) => true,
        Some(Ordering::Equal) => high_inclusive,
        Some(Ordering::Greater) => false,
        None => true,
    };

    above_low && below_high
}

fn split_ids(value: &str) -> Vec<String> {
    value.split([',', ' ']).filter(|d| !d.is_empty()).map(str::to_owned).collect()
}

impl Launcher {
    fn chosen_ids(&mut self) -> Vec<String> {
        split_ids(&self.get("dependencies"))
    }

    fn toggle_dependency(&mut self, id: &str) {
        let mut ids = self.chosen_ids();
        match ids.iter().position(|chosen| chosen == id) {
            Some(at) => {
                ids.remove(at);
            }
            None => ids.push(id.to_owned()),
        }
        self.set("dependencies", &ids.join(","));
    }

    pub(crate) fn dependencies_badge(&mut self) -> String {
        format!("({})", self.chosen_ids().len())
    }

    fn ensure_catalogue(&mut self) {
        if self.dependencies.catalogue.is_some() {
            return;
        }

        self.dependencies.catalogue = Some(match Catalogue::load() {
            Ok(catalogue) => Ok(catalogue),
            Err(Error::Denied(_)) => Err("The dependency list needs the net:start.spring.io capability. Type the ids in the Project section.".into()),
            Err(error) => Err(format!("Could not load the catalogue: {error}")),
        });
    }

    fn catalogue_message(&mut self) -> Option<String> {
        if self.get("server").trim_end_matches('/') != START {
            return Some(format!("The dependency list is only available for {START}. Type the ids in the Project section."));
        }

        self.ensure_catalogue();
        match &self.dependencies.catalogue {
            Some(Err(message)) => Some(message.clone()),
            _ => None,
        }
    }

    pub(crate) fn dependencies_tree(&mut self) -> (View, String) {
        let chosen = self.chosen_ids();
        let message = self.catalogue_message();
        let boot = Some(self.get("bootVersion")).filter(|v| !v.is_empty());
        let catalogue = match (&message, &self.dependencies.catalogue) {
            (None, Some(Ok(catalogue))) => Some(catalogue),
            _ => None,
        };
        let boot = boot.or_else(|| catalogue.map(|c| c.default_boot.clone()).filter(|v| !v.is_empty()));
        let search = self.dependencies.search.clone();
        let needle = search.as_ref().map(|q| q.to_lowercase());
        let compatible = |d: &Dependency| match (&boot, &d.range) {
            (Some(boot), Some(range)) => in_range(boot, range),
            _ => true,
        };
        let matches = |d: &Dependency| {
            needle.as_ref().is_none_or(|n| d.name.to_lowercase().contains(n) || d.description.to_lowercase().contains(n))
        };

        let mut items = vec![TreeItem::new(SELECTED, &format!("Selected ({})", chosen.len())).icon("check")];
        for id in &chosen {
            let known = catalogue.and_then(|c| c.find(id));
            let label = known.map_or(id.as_str(), |d| d.name.as_str());
            let detail = match (known, &boot) {
                (Some(d), Some(boot)) if !compatible(d) => format!("(not compatible with {boot})"),
                (Some(d), _) => d.description.clone(),
                (None, _) => String::new(),
            };
            let mut item = TreeItem::new(&format!("{CHOSEN_PREFIX}{id}"), label).parent(SELECTED);
            if !detail.is_empty() {
                item = item.detail(&detail);
            }
            items.push(item);
        }

        let mut found = 0;
        if let Some(message) = &message {
            items.push(TreeItem::new("message", message).icon("info"));
        }
        for category in catalogue.into_iter().flat_map(|c| &c.categories) {
            let shown: Vec<&Dependency> = category.dependencies.iter().filter(|d| compatible(d) && matches(d)).collect();
            if shown.is_empty() {
                continue;
            }

            found += shown.len();
            let key = format!("{CATEGORY_PREFIX}{}", category.name);
            items.push(TreeItem::new(&key, &category.name).icon("folder"));
            for d in shown {
                let mut item = TreeItem::new(&format!("{DEPENDENCY_PREFIX}{}", d.id), &d.name).parent(&key);
                if !d.description.is_empty() {
                    item = item.detail(&d.description);
                }
                if chosen.contains(&d.id) {
                    item = item.icon("check");
                }
                items.push(item);
            }
        }

        self.apply_expansion(&mut items, search.is_some());
        let status = match &search {
            Some(query) => format!("{found} matches for \"{query}\""),
            None => format!("{} selected. Double-click a dependency to add or remove it.", chosen.len()),
        };

        let mut view = View::new(Body::Tree(items)).button(Button::new("search", "Search…").icon("search"));
        if search.is_some() {
            view = view.button(Button::new("clearSearch", "Clear search").icon("close"));
        }
        let view = view
            .button(Button::new("clearAll", "Clear all").icon("trash").enabled(!chosen.is_empty()))
            .button(Button::new("refreshCatalogue", "Refresh").icon("refresh"));

        (view, status)
    }

    fn apply_expansion(&self, items: &mut [TreeItem], searching: bool) {
        let parents: BTreeSet<String> = items.iter().filter_map(|i| i.parent.clone()).collect();
        for item in items.iter_mut() {
            item.expandable = parents.contains(&item.key);
            item.expanded = item.expandable
                && self.dependencies.expansion.get(&item.key).copied().unwrap_or(item.key == SELECTED || searching);
        }
    }

    /// True when the event belonged to the Dependencies section.
    pub(crate) fn handle_dependencies(&mut self, event: &ViewEvent) -> Result<bool, Error> {
        match event {
            ViewEvent::Expand(key) => {
                self.dependencies.expansion.insert(key.clone(), true);
            }
            ViewEvent::Collapse(key) => {
                self.dependencies.expansion.insert(key.clone(), false);
            }
            ViewEvent::Activate(key) => {
                let Some(id) = key.strip_prefix(DEPENDENCY_PREFIX).or_else(|| key.strip_prefix(CHOSEN_PREFIX)) else {
                    return Ok(false);
                };
                self.toggle_dependency(id);
            }
            ViewEvent::Button(b) if b == "search" => match host::input("Search dependencies", self.dependencies.search.as_deref()) {
                Ok(Some(query)) => self.dependencies.search = Some(query.trim().to_owned()).filter(|q| !q.is_empty()),
                Ok(None) | Err(Error::Invalid(_)) => {}
                Err(error) => return Err(error),
            },
            ViewEvent::Button(b) if b == "clearSearch" => self.dependencies.search = None,
            ViewEvent::Button(b) if b == "clearAll" => match host::confirm("Clear all dependencies", "Remove every chosen dependency?") {
                Ok(true) => self.set("dependencies", ""),
                Ok(false) | Err(Error::Invalid(_)) => {}
                Err(error) => return Err(error),
            },
            ViewEvent::Button(b) if b == "refreshCatalogue" => self.dependencies.catalogue = None,
            _ => return Ok(false),
        }

        Ok(true)
    }
}
