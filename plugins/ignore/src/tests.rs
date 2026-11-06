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
    let _host = MockHost::install();
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
fn deleting_a_user_template_drops_its_star() {
    let host = MockHost::install();
    host::storage::set("template:PHP", "/vendor/\n").unwrap();
    host::storage::set("starred:u:PHP", "1").unwrap();
    host::storage::set("starred:t:Rust", "1").unwrap();
    let mut plugin = IgnorePlugin::default();
    let ctx = host.invocation();
    plugin.handle("templates", ViewEvent::Select("u:PHP".into()), &ctx).unwrap();

    plugin.handle("templates", ViewEvent::Button("delete".into()), &ctx).unwrap();
    plugin.handle("templates", ViewEvent::Button("confirm-delete".into()), &ctx).unwrap();

    assert!(host.storage().get("starred:u:PHP").is_none());
    assert_eq!(host.storage().get("starred:t:Rust").map(String::as_str), Some("1"));
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
    assert_eq!(enabled(&mut plugin), [false, false, false]);

    host.open_editor("/project/.gitignore", "ignore", "", &[(0, 0, 0, 0)]);
    plugin.handle("templates", ViewEvent::Select("t:Rust".into()), &ctx).unwrap();
    assert_eq!(enabled(&mut plugin), [true, false, true]);
    plugin.handle("templates", ViewEvent::Select("u:PHP".into()), &ctx).unwrap();
    assert_eq!(enabled(&mut plugin), [true, true, true]);
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
    let items = template_tree(&[], &HashSet::new(), &HashSet::new());
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

#[test]
fn file_templates_create_files_the_plugin_recognises() {
    let manifest = include_str!("../plugin.toml");
    let language = include_str!("../languages/dotignore.toml");
    let section = manifest.split("[[contributes.fileTemplates]]").nth(1).expect("a file template");
    let value = |key: &str| {
        let line = section.lines().find(|l| l.starts_with(&format!("{key} = "))).unwrap();
        line.split('"').nth(1).unwrap().to_owned()
    };

    let extension = value("extension");
    let extensions = language.lines().find(|l| l.starts_with("extensions = ")).unwrap();
    assert!(extensions.contains(&format!("\"{extension}\"")), "{extension} is not a language extension");

    let body = std::fs::read_to_string(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), value("path"))).unwrap();
    assert!(!body.trim().is_empty());
}

fn tree(plugin: &mut IgnorePlugin, host: &MockHost) -> Vec<TreeItem> {
    let View { body: Body::Tree(items), .. } = plugin.render("templates", None, &host.invocation()).unwrap() else { panic!() };
    items
}

fn unignore_edit(content: &str) -> Vec<CodeAction> {
    let host = MockHost::install();
    host.add_file(".gitignore", content);
    IgnorePlugin::default().code_actions(&doc("/project/src/a.rs", ""), Range::default())
}

#[test]
fn unignore_action_is_absent_when_the_setting_is_off() {
    let host = MockHost::install();
    host.add_file(".gitignore", "/target/\n");
    host.set_setting("unignoreActions", "false");
    let actions = IgnorePlugin::default().code_actions(&doc("/project/src/a.rs", ""), Range::default());
    assert_eq!(actions.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(), ["Add /src/a.rs to .gitignore"]);
}

#[test]
fn unignore_action_is_offered_by_default() {
    let titles: Vec<String> = unignore_edit("/target/\n").into_iter().map(|a| a.title).collect();
    assert_eq!(titles, ["Add /src/a.rs to .gitignore", "Add !/src/a.rs to .gitignore (unignore)"]);
}

#[test]
fn star_button_toggles_and_persists_a_template() {
    let host = MockHost::install();
    let mut plugin = IgnorePlugin::default();
    let ctx = host.invocation();
    plugin.handle("templates", ViewEvent::Select("t:Rust".into()), &ctx).unwrap();
    plugin.handle("templates", ViewEvent::Button("star".into()), &ctx).unwrap();
    assert_eq!(host.storage().get("starred:t:Rust").map(String::as_str), Some("1"));
    let label = |plugin: &mut IgnorePlugin| plugin.render("templates", None, &ctx).unwrap().toolbar[2].label.clone();
    assert_eq!(label(&mut plugin), "Unstar");
    plugin.handle("templates", ViewEvent::Button("star".into()), &ctx).unwrap();
    assert!(host.storage().get("starred:t:Rust").is_none());
    assert_eq!(label(&mut plugin), "Star");
}

#[test]
fn starred_templates_are_listed_under_starred_and_marked() {
    let host = MockHost::install();
    assert!(tree(&mut IgnorePlugin::default(), &host).iter().all(|i| i.key != "g:starred"));

    host::storage::set("starred:t:Rust", "1").unwrap();
    let items = tree(&mut IgnorePlugin::default(), &host);
    assert!(items.iter().any(|i| i.key == "g:starred"));
    let listed = items.iter().find(|i| i.parent.as_deref() == Some("g:starred")).unwrap();
    assert_eq!(listed.icon.as_deref(), Some("star"));
    assert_eq!(items.iter().find(|i| i.key == "t:Rust").unwrap().icon.as_deref(), Some("star"));
    let keys: HashSet<&str> = items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys.len(), items.len(), "unique keys");
}

#[test]
fn the_star_button_is_disabled_without_a_template_selected() {
    let host = MockHost::install();
    let mut plugin = IgnorePlugin::default();
    let ctx = host.invocation();
    plugin.handle("templates", ViewEvent::Select("g:Global".into()), &ctx).unwrap();
    assert!(!plugin.render("templates", None, &ctx).unwrap().toolbar[2].enabled);
    assert!(plugin.handle("templates", ViewEvent::Button("star".into()), &ctx).is_err());
    assert!(host.storage().is_empty());
}

#[test]
fn starring_a_deleted_user_template_is_dropped_on_render() {
    let host = MockHost::install();
    host::storage::set("starred:u:Gone", "1").unwrap();
    let items = tree(&mut IgnorePlugin::default(), &host);
    assert!(items.iter().all(|i| i.key != "g:starred" && !i.key.contains("Gone")));
}

#[test]
fn unignore_edit_uses_crlf_in_a_crlf_gitignore() {
    let content = "/target/\r\n";
    let actions = unignore_edit(content);
    let edit = &actions[1].edits[0].1[0];
    assert_eq!(edit.text, "!/src/a.rs\r\n");
    assert!(!edit.text.replace("\r\n", "").contains('\n'));
}

#[test]
fn template_insertion_uses_crlf_in_a_crlf_file() {
    let host = MockHost::install();
    host.open_editor("/project/.gitignore", "ignore", "/target/\r\n", &[(0, 0, 0, 0)]);
    IgnorePlugin::default().handle("templates", ViewEvent::Activate("t:Rust".into()), &host.invocation()).unwrap();
    let text = host.editor_text();
    assert!(text.starts_with("/target/\r\n\r\n### Rust template\r\n"), "{text:?}");
    assert!(!text.replace("\r\n", "").contains('\n'));
}

#[test]
fn appending_to_a_file_without_final_newline_adds_one_crlf() {
    let edit = append("a\r\nb", "c\r\n", false);
    assert_eq!(apply("a\r\nb", &edit), "a\r\nb\r\nc\r\n");
    assert_eq!(append("a", "c\n", false).text, "\nc\n");
}

#[test]
fn unignore_ranges_use_utf16_columns_with_multibyte_text() {
    let actions = unignore_edit("/\u{fc}\u{1F600}");
    let range = actions[0].edits[0].1[0].range;
    assert_eq!(range.start, Position { line: 0, character: 4 });
}

#[test]
fn a_missing_gitignore_offers_no_action() {
    let host = MockHost::install();
    assert!(IgnorePlugin::default().code_actions(&doc("/project/src/a.rs", ""), Range::default()).is_empty());
    drop(host);
}

#[test]
fn selected_paths_are_appended_to_the_open_ignore_file() {
    let host = MockHost::install();
    host.open_editor("/project/.dockerignore", "ignore", "/dist\n", &[(0, 0, 0, 0)]);
    host.select_paths(&["/project/dist", "/project/src/a[1].rs", "/project/node_modules", "/elsewhere/x"]);

    IgnorePlugin::default().run_command("add-selected", &host.invocation(), None).unwrap();

    assert_eq!(host.editor_text(), "/dist\n/src/a\\[1\\].rs\n/node_modules\n");
}

#[test]
fn selected_paths_need_an_open_ignore_file() {
    let host = MockHost::install();
    host.open_editor("/project/main.rs", "rust", "", &[(0, 0, 0, 0)]);
    host.select_paths(&["/project/dist"]);

    assert!(IgnorePlugin::default().run_command("add-selected", &host.invocation(), None).is_err());
}

#[test]
fn selected_paths_go_to_the_picked_root_ignore_file() {
    let host = MockHost::install();
    host.open_editor("/project/.dockerignore", "ignore", "/dist\n", &[(0, 0, 0, 0)]);
    host.add_file("/project/.gitignore", "");
    host.select_paths(&["/project/build"]);
    host.answer_next_prompt(Some(&["/project/.gitignore"]));

    let message = IgnorePlugin::default().run_command("add-selected", &host.invocation(), None).unwrap();

    assert_eq!(host.opened_locations(), [("/project/.gitignore".to_owned(), 0, 0)]);
    assert!(message.unwrap().starts_with("Opened /project/.gitignore"));
    assert_eq!(host.editor_text(), "/dist\n");
}

#[test]
fn picking_the_focused_ignore_file_edits_it() {
    let host = MockHost::install();
    host.open_editor("/project/.dockerignore", "ignore", "/dist\n", &[(0, 0, 0, 0)]);
    host.add_file("/project/.gitignore", "");
    host.select_paths(&["/project/build"]);
    host.answer_next_prompt(Some(&["/project/.dockerignore"]));

    IgnorePlugin::default().run_command("add-selected", &host.invocation(), None).unwrap();

    assert_eq!(host.editor_text(), "/dist\n/build\n");
}

#[test]
fn dismissing_the_ignore_file_pick_changes_nothing() {
    let host = MockHost::install();
    host.open_editor("/project/.dockerignore", "ignore", "/dist\n", &[(0, 0, 0, 0)]);
    host.add_file("/project/.gitignore", "");
    host.select_paths(&["/project/build"]);

    let message = IgnorePlugin::default().run_command("add-selected", &host.invocation(), None).unwrap();

    assert_eq!(message, None);
    assert_eq!(host.editor_text(), "/dist\n");
}

#[test]
fn completes_entries_from_the_files_below_the_ignore_file() {
    let host = MockHost::install();
    host.set_project_root(Some("/project"));
    host.add_file("/project/web/.gitignore", "");
    host.add_file("/project/web/dist/app.js", "");
    host.add_file("/project/web/dist/vendor/lib.js", "");
    host.add_file("/project/web/docs.md", "");
    host.add_file("/project/web/src/main.ts", "");
    let mut plugin = IgnorePlugin::default();
    let labels = |plugin: &mut IgnorePlugin, text: &str| -> Vec<String> {
        let at = Position::new(0, text.len() as u32);
        plugin.completion(&doc("/project/web/.gitignore", text), at, None).into_iter().map(|i| i.label).collect()
    };

    assert_eq!(labels(&mut plugin, "d"), ["dist/", "docs.md"]);
    assert_eq!(labels(&mut plugin, "!/dist/"), ["app.js", "vendor/"]);
    assert_eq!(labels(&mut plugin, "dist/v"), ["vendor/"]);
    assert!(labels(&mut plugin, "dist/*").is_empty());
    assert!(labels(&mut plugin, "# d").is_empty());
    assert!(labels(&mut plugin, "../x").is_empty());

    let item = plugin.completion(&doc("/project/web/.gitignore", "dist/ap"), Position::new(0, 7), None).remove(0);
    assert_eq!(item.replace, Some(Range::new(Position::new(0, 5), Position::new(0, 7))));
}

#[test]
fn regexp_ignore_files_get_no_path_completion() {
    let host = MockHost::install();
    host.set_project_root(Some("/project"));
    host.add_file("/project/build/a.o", "");
    let mut plugin = IgnorePlugin::default();

    assert!(plugin.completion(&doc("/project/.boringignore", "b"), Position::new(0, 1), None).is_empty());
    assert!(plugin.completion(&doc("/project/.hgignore", "b"), Position::new(0, 1), None).is_empty());
}
