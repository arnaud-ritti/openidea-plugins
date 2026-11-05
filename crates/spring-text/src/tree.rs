//! Open and closed nodes of a tool window tree. The IDE shows a node open
//! only when the plugin renders it `expanded`, so a click on an expander does
//! nothing until the plugin records it and refreshes the view.

use std::collections::{BTreeMap, BTreeSet};

use openidea_plugin::views::{TreeItem, ViewEvent};

// ponytail: kept in memory, so nodes fall back to their defaults when the instance is reclaimed (5 min idle); move to host::storage if that annoys.
#[derive(Default)]
pub struct Expansion(BTreeMap<String, bool>);

impl Expansion {
    /// Makes the items with children expandable, open as the user left it, or as
    /// rendered (`expanded`) until the user toggles it. Items without children
    /// aren't: the IDE shows an expandable item with no children as still loading.
    pub fn apply(&self, items: &mut [TreeItem]) {
        let parents: BTreeSet<String> = items.iter().filter_map(|i| i.parent.clone()).collect();

        for item in items.iter_mut() {
            item.expandable = parents.contains(&item.key);

            if !item.expandable {
                item.expanded = false;
                continue;
            }

            if let Some(&open) = self.0.get(&item.key) {
                item.expanded = open;
            }
        }
    }

    /// Records an expand or collapse; true when `event` was one, and the view needs a refresh.
    pub fn handle(&mut self, event: &ViewEvent) -> bool {
        match event {
            ViewEvent::Expand(key) => self.0.insert(key.clone(), true),
            ViewEvent::Collapse(key) => self.0.insert(key.clone(), false),
            _ => return false,
        };

        true
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Vec<TreeItem> {
        vec![
            TreeItem::new("group", "Group").expandable(true),
            TreeItem::new("parent", "Parent").parent("group"),
            TreeItem::new("child", "Child").parent("parent"),
        ]
    }

    #[test]
    fn items_with_children_become_expandable_and_keep_their_default() {
        let mut items = tree();

        Expansion::default().apply(&mut items);

        assert!(items[0].expandable && items[0].expanded);
        assert!(items[1].expandable && !items[1].expanded);
        assert!(!items[2].expandable);
    }

    #[test]
    fn items_without_children_are_never_expandable() {
        let mut items = vec![TreeItem::new("empty", "Empty module").expandable(true)];

        Expansion::default().apply(&mut items);

        assert!(!items[0].expandable && !items[0].expanded);
    }

    #[test]
    fn expand_and_collapse_are_remembered() {
        let mut expansion = Expansion::default();
        assert!(expansion.handle(&ViewEvent::Expand("parent".into())));
        assert!(expansion.handle(&ViewEvent::Collapse("group".into())));
        assert!(!expansion.handle(&ViewEvent::Activate("child".into())));
        let mut items = tree();

        expansion.apply(&mut items);

        assert!(!items[0].expanded);
        assert!(items[1].expanded);
    }
}
