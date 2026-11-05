use super::*;
use check::{Dialect, Syntax};
use openidea_plugin::Plugin;
use openidea_plugin::mock::MockHost;

const GIT: Dialect = Dialect { syntax: Syntax::Glob, switchable: false };
const HG: Dialect = Dialect { syntax: Syntax::Regexp, switchable: true };

fn doc(path: &str, text: &str) -> Document {
    Document { path: path.into(), language: "ignore".into(), version: 1, text: text.into() }
}

fn apply(text: &str, edit: &TextEdit) -> String {
    let start = text::offset(text, edit.range.start).unwrap();
    let end = text::offset(text, edit.range.end).unwrap();
    format!("{}{}{}", &text[..start], edit.text, &text[end..])
}

#[test]
fn removing_a_duplicate_deletes_its_line() {
    let text = "a\nb\na";
    let fixes = quick_fixes(&doc("/p/.gitignore", text), 2, GIT);
    assert_eq!(fixes[0].title, "Remove entry");
    assert_eq!(apply(text, &fixes[0].edits[0].1[0]), "a\nb");
    let text = "a\na\nb\n";
    let fixes = quick_fixes(&doc("/p/.gitignore", text), 1, GIT);
    assert_eq!(apply(text, &fixes[0].edits[0].1[0]), "a\nb\n");
}

#[test]
fn invalid_syntax_values_offer_both_syntaxes() {
    let titles: Vec<String> =
        quick_fixes(&doc("/p/.hgignore", "syntax: perl\n"), 0, HG).into_iter().map(|a| a.title).collect();
    assert_eq!(titles, ["Use glob syntax", "Use regexp syntax"]);
}

#[test]
fn completes_syntax_values_in_hgignore_only() {
    let mut plugin = IgnorePlugin::default();
    let items = plugin.completion(&doc("/p/.hgignore", "syntax: g"), Position::new(0, 9), None);
    assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["glob", "regexp", "rootglob"]);
    assert!(plugin.completion(&doc("/p/.gitignore", "syntax: g"), Position::new(0, 9), None).is_empty());
    assert!(plugin.completion(&doc("/p/.hgignore", "build/"), Position::new(0, 6), None).is_empty());
}

#[test]
fn publishes_problems_of_opened_ignore_files() {
    let host = MockHost::install();
    host.add_file(".gitignore", "a\na\n*.log\n/debug.log\n");
    host.add_file("src/a.rs", "a\na\n");
    IgnorePlugin::default().handle_events(
        &[Event::FileOpened("/project/.gitignore".into()), Event::FileSaved("/project/src/a.rs".into())],
        &host.invocation(),
    );
    let diagnostics = host.diagnostics();
    assert_eq!(diagnostics.len(), 1);
    let severities: Vec<(u32, Severity)> =
        diagnostics["/project/.gitignore"].iter().map(|d| (d.range.start.line, d.severity)).collect();
    assert_eq!(severities, [(1, Severity::Error), (3, Severity::Warning)]);
}

#[test]
fn offers_add_to_gitignore_for_project_files() {
    let host = MockHost::install();
    host.add_file(".gitignore", "/target/\n/out\\[1\\].log");
    let mut plugin = IgnorePlugin::default();
    let actions = plugin.code_actions(&doc("/project/out[1].log", ""), Range::default());
    assert_eq!(actions.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(), ["Add !/out\\[1\\].log to .gitignore (unignore)"]);

    let actions = plugin.code_actions(&doc("/project/src/main.rs", ""), Range::default());
    let (path, edits) = &actions[0].edits[0];
    assert_eq!(path, "/project/.gitignore");
    assert_eq!(apply("/target/\n/out\\[1\\].log", &edits[0]), "/target/\n/out\\[1\\].log\n/src/main.rs\n");
}

#[test]
fn entries_naming_a_file_go_to_it() {
    let host = MockHost::install();
    host.add_file("app/config/local.php", "<?php");
    let text = "*.log\n/config/local.php\nconfig/\nmissing.txt\n";
    let mut plugin = IgnorePlugin::default();
    let mut target = |line| plugin.definition(&doc("/project/app/.gitignore", text), Position::new(line, 2));
    assert_eq!(target(1)[0].path, "/project/app/config/local.php");
    for line in [0, 2, 3] {
        assert!(target(line).is_empty(), "line {line}");
    }
}

#[test]
fn status_bar_tells_when_the_focused_file_is_ignored() {
    let host = MockHost::install();
    host.add_file(".gitignore", "*.log\n!keep.log\n");
    host.add_file("app/.gitignore", "cache/\n");
    let focus = |path: &str| {
        IgnorePlugin::default().handle_events(&[Event::EditorFocused(Some(path.into()))], &host.invocation());
        host.status_items().get(IGNORED_STATUS).and_then(|i| i.tooltip.clone())
    };

    assert_eq!(
        focus("/project/app/cache/data.bin").as_deref(),
        Some("This file is ignored by `cache/` in app/.gitignore.")
    );
    assert!(focus("/project/keep.log").is_none());
    assert!(focus("/project/debug.log").is_some());
    host.set_setting("ignoredEditing", "false");
    assert!(focus("/project/debug.log").is_none());
}

#[test]
fn warns_once_about_a_git_project_without_gitignore() {
    let host = MockHost::install();
    host.add_file(".git/HEAD", "ref: refs/heads/main\n");
    let open = || IgnorePlugin::default().handle_events(&[Event::FileOpened("/project/a.rs".into())], &host.invocation());
    open();
    open();
    assert_eq!(host.notifications().len(), 1);
    assert_eq!(host.notifications()[0].title, "No .gitignore file in this Git project");
}

#[test]
fn no_missing_gitignore_warning_outside_git_or_with_one() {
    let host = MockHost::install();
    IgnorePlugin::default().handle_events(&[Event::FileOpened("/project/a.rs".into())], &host.invocation());
    host.add_file(".git/HEAD", "ref: refs/heads/main\n");
    host.add_file(".gitignore", "");
    IgnorePlugin::default().handle_events(&[Event::FileOpened("/project/a.rs".into())], &host.invocation());
    assert!(host.notifications().is_empty());
}

#[test]
fn appends_a_template_to_the_open_ignore_file() {
    let host = MockHost::install();
    host.open_editor("/project/.gitignore", "ignore", "/target/", &[(0, 0, 0, 0)]);
    IgnorePlugin::default().handle("templates", ViewEvent::Activate("t:Rust".into()), &host.invocation()).unwrap();
    assert!(host.editor_text().starts_with("/target/\n\n### Rust template\n"), "{}", host.editor_text());

    host.open_editor("/project/main.rs", "rust", "", &[(0, 0, 0, 0)]);
    assert!(IgnorePlugin::default().handle("templates", ViewEvent::Activate("t:Rust".into()), &host.invocation()).is_err());
}

#[test]
fn inserts_a_template_at_the_caret_when_set() {
    let host = MockHost::install();
    host.set_setting("insertAtCursor", "true");
    host.open_editor("/project/.gitignore", "ignore", "a\nb\n", &[(1, 1, 1, 1)]);
    IgnorePlugin::default().handle("templates", ViewEvent::Activate("t:Global/macOS".into()), &host.invocation()).unwrap();
    assert!(host.editor_text().starts_with("a\n### macOS template\n"), "{}", host.editor_text());
    assert!(host.editor_text().ends_with("\n\nb\n"));
}

#[test]
fn user_templates_are_created_listed_used_and_deleted() {
    let host = MockHost::install();
    host.open_editor("/project/.gitignore", "ignore", "/vendor/\n", &[(0, 0, 0, 0)]);
    let mut plugin = IgnorePlugin::default();
    let ctx = host.invocation();
    plugin.handle("templates", ViewEvent::Button("new".into()), &ctx).unwrap();
    assert_eq!(plugin.naming.as_deref(), Some(".gitignore"));
    plugin.handle("templates", ViewEvent::Field("name".into(), "PHP".into()), &ctx).unwrap();
    plugin.handle("templates", ViewEvent::Button("save".into()), &ctx).unwrap();
    assert_eq!(host.storage().get("template:PHP").map(String::as_str), Some("/vendor/\n"));

    let View { body: Body::Tree(items), .. } = plugin.render("templates", None, &ctx).unwrap() else { panic!() };
    let user = items.iter().find(|i| i.key == "u:PHP").unwrap();
    assert_eq!(user.parent.as_deref(), Some("g:user"));

    plugin.handle("templates", ViewEvent::Activate("u:PHP".into()), &ctx).unwrap();
    assert_eq!(host.editor_text(), "/vendor/\n\n### PHP template\n/vendor/\n");

    plugin.handle("templates", ViewEvent::Select("u:PHP".into()), &ctx).unwrap();
    plugin.handle("templates", ViewEvent::Button("delete".into()), &ctx).unwrap();
    assert_eq!(plugin.render("templates", None, &ctx).unwrap().body, Body::Message("Delete user template PHP?".into()));
    assert!(!host.storage().is_empty(), "nothing deleted before confirming");
    plugin.handle("templates", ViewEvent::Button("confirm-delete".into()), &ctx).unwrap();
    assert!(host.storage().is_empty());
    assert_eq!(host.notifications().last().unwrap().body, "Deleted user template PHP.");
}

#[test]
fn cancelling_a_deletion_keeps_the_template() {
    let host = MockHost::install();
    host::storage::set("template:PHP", "/vendor/\n").unwrap();
    let mut plugin = IgnorePlugin::default();
    let ctx = host.invocation();
    plugin.handle("templates", ViewEvent::Select("u:PHP".into()), &ctx).unwrap();
    plugin.handle("templates", ViewEvent::Button("delete".into()), &ctx).unwrap();
    plugin.handle("templates", ViewEvent::Button("cancel".into()), &ctx).unwrap();
    assert!(matches!(plugin.render("templates", None, &ctx).unwrap().body, Body::Tree(_)));
    assert_eq!(host.storage().len(), 1);
}

#[test]
fn refuses_to_overwrite_a_user_template() {
    let host = MockHost::install();
    host::storage::set("template:PHP", "old\n").unwrap();
    host.open_editor("/project/.gitignore", "ignore", "new\n", &[(0, 0, 0, 0)]);
    let mut plugin = IgnorePlugin { naming: Some("PHP".into()), ..Default::default() };
    let err = plugin.handle("templates", ViewEvent::Button("save".into()), &host.invocation()).unwrap_err();
    assert_eq!(err.to_string(), "A user template named PHP already exists.");
    assert_eq!(host.storage()["template:PHP"], "old\n");
}

#[test]
fn toolbar_buttons_follow_the_editor_and_selection() {
    let host = MockHost::install();
    host::storage::set("template:PHP", "/vendor/\n").unwrap();
    let mut plugin = IgnorePlugin::default();
    let ctx = host.invocation();
    let enabled = |plugin: &mut IgnorePlugin| -> Vec<bool> {
        plugin.render("templates", None, &ctx).unwrap().toolbar.iter().map(|b| b.enabled).collect()
    };
    assert_eq!(enabled(&mut plugin), [false, false]);

    host.open_editor("/project/.gitignore", "ignore", "", &[(0, 0, 0, 0)]);
    plugin.handle("templates", ViewEvent::Select("t:Rust".into()), &ctx).unwrap();
    assert_eq!(enabled(&mut plugin), [true, false]);
    plugin.handle("templates", ViewEvent::Select("u:PHP".into()), &ctx).unwrap();
    assert_eq!(enabled(&mut plugin), [true, true]);
    assert_eq!(host.refreshed_views(), ["templates", "templates"]);
}

#[test]
fn expanding_a_group_redraws_the_view() {
    let host = MockHost::install();
    let mut plugin = IgnorePlugin::default();
    let ctx = host.invocation();
    let expanded = |plugin: &mut IgnorePlugin| {
        let View { body: Body::Tree(items), .. } = plugin.render("templates", None, &ctx).unwrap() else { panic!() };
        items.iter().find(|i| i.key == "g:Global").unwrap().expanded
    };
    plugin.handle("templates", ViewEvent::Expand("g:Global".into()), &ctx).unwrap();
    assert!(expanded(&mut plugin));
    plugin.handle("templates", ViewEvent::Collapse("g:Global".into()), &ctx).unwrap();
    assert!(!expanded(&mut plugin));
    assert_eq!(host.refreshed_views(), ["templates", "templates"]);
}

#[test]
fn template_tree_nests_community_folders() {
    let items = template_tree(&[], &HashSet::new());
    let parent = |key: &str| items.iter().find(|i| i.key == key).and_then(|i| i.parent.clone());
    assert_eq!(parent("t:Rust").as_deref(), Some("g:"));
    assert_eq!(parent("t:Global/macOS").as_deref(), Some("g:Global"));
    assert_eq!(parent("g:community/DotNet").as_deref(), Some("g:community"));
    let keys: HashSet<&str> = items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys.len(), items.len(), "unique keys");
    assert!(items.iter().filter_map(|i| i.parent.as_deref()).all(|p| keys.contains(p)), "every parent exists");
}

#[test]
fn embeds_the_templates() {
    assert!(TEMPLATES.len() > 250);
    assert!(TEMPLATES.iter().any(|(n, _)| *n == "Global/macOS"));
}
