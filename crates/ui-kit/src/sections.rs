use openidea_plugin::host::storage;
use openidea_plugin::views::{Button, View, ViewEvent};

const BUTTON_PREFIX: &str = "section:";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionDef {
    pub id: &'static str,
    pub label: &'static str,
}

pub struct Switcher {
    storage_key: &'static str,
    defs: &'static [SectionDef],
    active: &'static str,
}

impl Switcher {
    pub fn new(storage_key: &'static str, defs: &'static [SectionDef]) -> Switcher {
        let stored = storage::get(storage_key).ok().flatten();
        let active = defs
            .iter()
            .find(|def| Some(def.id) == stored.as_deref())
            .or(defs.first())
            .map_or("", |def| def.id);
        Switcher { storage_key, defs, active }
    }

    pub fn active(&self) -> &'static str {
        self.active
    }

    pub fn set(&mut self, id: &str) -> bool {
        let Some(def) = self.defs.iter().find(|def| def.id == id) else {
            return false;
        };
        self.active = def.id;
        let _ = storage::set(self.storage_key, def.id);
        true
    }

    /// The host has no pressed state for toolbar buttons, so the active section gets a bullet.
    pub fn buttons(&self, view: View, badges: &[(&str, String)]) -> View {
        let mut toolbar: Vec<Button> = self
            .defs
            .iter()
            .map(|def| {
                let marker = if def.id == self.active { "\u{25cf} " } else { "" };
                let badge = badges
                    .iter()
                    .find(|(id, _)| *id == def.id)
                    .map_or(String::new(), |(_, badge)| format!(" {badge}"));
                Button::new(&format!("{BUTTON_PREFIX}{}", def.id), &format!("{marker}{}{badge}", def.label))
            })
            .collect();
        toolbar.extend(view.toolbar);
        View { toolbar, ..view }
    }

    pub fn handle(&mut self, event: &ViewEvent) -> bool {
        let ViewEvent::Button(id) = event else {
            return false;
        };
        let Some(section) = id.strip_prefix(BUTTON_PREFIX) else {
            return false;
        };
        self.set(section);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::views::Body;

    const DEFS: &[SectionDef] = &[
        SectionDef { id: "http", label: "HTTP" },
        SectionDef { id: "ws", label: "WebSocket" },
    ];

    #[test]
    fn new_falls_back_to_the_first_section_for_an_unknown_stored_id() {
        let host = MockHost::install();
        storage::set("k", "gone").unwrap();
        assert_eq!(Switcher::new("k", DEFS).active(), "http");
        drop(host);
    }

    #[test]
    fn set_stores_the_active_section() {
        let host = MockHost::install();
        let mut switcher = Switcher::new("k", DEFS);
        assert!(switcher.set("ws"));
        assert!(!switcher.set("nope"));
        assert_eq!(switcher.active(), "ws");
        assert_eq!(host.storage().get("k").map(String::as_str), Some("ws"));
        assert_eq!(Switcher::new("k", DEFS).active(), "ws");
    }

    #[test]
    fn buttons_mark_the_active_section_and_show_badges() {
        let _host = MockHost::install();
        let mut switcher = Switcher::new("k", DEFS);
        switcher.set("ws");
        let view = View::new(Body::Message("x".into())).button(Button::new("refresh", "Refresh"));
        let toolbar = switcher.buttons(view, &[("http", "(3)".to_string())]).toolbar;
        let shown: Vec<(&str, &str)> = toolbar.iter().map(|b| (b.id.as_str(), b.label.as_str())).collect();
        assert_eq!(
            shown,
            [("section:http", "HTTP (3)"), ("section:ws", "\u{25cf} WebSocket"), ("refresh", "Refresh")]
        );
    }

    #[test]
    fn handle_consumes_only_section_buttons() {
        let _host = MockHost::install();
        let mut switcher = Switcher::new("k", DEFS);
        assert!(!switcher.handle(&ViewEvent::Button("refresh".into())));
        assert!(!switcher.handle(&ViewEvent::Select("section:ws".into())));
        assert_eq!(switcher.active(), "http");
        assert!(switcher.handle(&ViewEvent::Button("section:ws".into())));
        assert_eq!(switcher.active(), "ws");
    }
}
