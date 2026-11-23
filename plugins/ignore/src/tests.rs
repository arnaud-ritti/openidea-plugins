use super::*;
use check::{Dialect, Syntax};
use openidea_plugin::Plugin;
use openidea_plugin::file_actions::{FileActions,FileAction};
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

fn add_actions(plugin:&mut IgnorePlugin,host:&MockHost,doc:&Document)->Vec<FileAction>{
    host.open_editor(&doc.path,&doc.language,&doc.text,&[]);
    let actions=plugin.file_actions(doc,Range::default()).into_iter().filter(|a|a.title.starts_with("Add ")).collect::<Vec<_>>();
    actions.into_iter().map(|a|{host.answer_next_prompt(Some(&[]));plugin.resolve_file_action(doc,a).unwrap()}).collect()
}

#[test]
fn removing_a_duplicate_deletes_its_line() {
    let text = "a\nb\na";
    let fixes = quick_fixes(&doc("/p/.gitignore", text), 2, check(text, GIT));
    assert_eq!(fixes[0].title, "Remove entry");
    assert_eq!(apply(text, &fixes[0].edits[0].1[0]), "a\nb");
    let text = "a\na\nb\n";
    let fixes = quick_fixes(&doc("/p/.gitignore", text), 1, check(text, GIT));
    assert_eq!(apply(text, &fixes[0].edits[0].1[0]), "a\nb\n");
}

#[test]
fn invalid_syntax_values_offer_both_syntaxes() {
    let titles: Vec<String> =
        quick_fixes(&doc("/p/.hgignore", "syntax: perl\n"), 0, check("syntax: perl\n", HG)).into_iter().map(|a| a.title).collect();
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
fn ignore_problem_reported_once() {
    let host = MockHost::install();
    let text = "a\na\n*.log\n/debug.log\n";
    host.add_file(".gitignore", text);
    host.open_editor("/project/.gitignore", "ignore", text, &[]);
    let mut plugin = IgnorePlugin::default();
    plugin.handle_events(&[Event::FileOpened("/project/.gitignore".into())], &host.invocation());
    host.refresh_diagnostics(&mut plugin);

    assert!(host.diagnostics().is_empty());
    let severities: Vec<(u32, Severity)> =
        host.provider_diagnostics()["/project/.gitignore"].iter().map(|d| (d.range.start.line, d.severity)).collect();
    assert_eq!(severities, [(1, Severity::Error), (3, Severity::Warning)]);
}

fn provider_lines(host: &MockHost, plugin: &mut IgnorePlugin, path: &str, text: &str) -> Vec<(u32, Option<String>)> {
    host.open_editor(path, "ignore", text, &[]);
    host.refresh_diagnostics(plugin);
    host.provider_diagnostics().get(path).map(|d| d.iter().map(|d| (d.range.start.line, d.code.clone())).collect()).unwrap_or_default()
}

#[test]
fn large_gitignore_shows_duplicates_through_the_provider() {
    let host = MockHost::install();
    let text = format!("{}a\na\n", (0..600).map(|i| format!("f{i}\n")).collect::<String>());
    let found = provider_lines(&host, &mut IgnorePlugin::default(), "/project/.gitignore", &text);
    assert!(found.contains(&(601, Some("duplicate-entry".into()))));
}

#[test]
fn unsaved_edit_introducing_a_duplicate_shows_it() {
    let host = MockHost::install();
    host.add_file(".gitignore", "a\nb\n");
    let mut plugin = IgnorePlugin::default();
    plugin.handle_events(&[Event::FileOpened("/project/.gitignore".into())], &host.invocation());
    let found = provider_lines(&host, &mut plugin, "/project/.gitignore", "a\nb\na\n");
    assert!(found.contains(&(2, Some("duplicate-entry".into()))));
}

#[test]
fn opening_nine_ignore_files_still_serves_the_first() {
    let host = MockHost::install();
    let mut plugin = IgnorePlugin::default();
    for i in 0..9 {
        host.add_file(&format!("d{i}/.gitignore"), "a\na\n");
        plugin.handle_events(&[Event::FileOpened(format!("/project/d{i}/.gitignore"))], &host.invocation());
    }
    assert_eq!(plugin.cached_checks.len(), 8);
    assert!(plugin.cached_checks.iter().all(|(p, _, _)| p != "/project/d0/.gitignore"));
    assert!(plugin.cached_checks.iter().any(|(p, _, _)| p == "/project/d8/.gitignore"));
    let found = provider_lines(&host, &mut plugin, "/project/d0/.gitignore", "a\na\n");
    assert_eq!(plugin.cached_checks.len(), 8);
    assert!(found.contains(&(1, Some("duplicate-entry".into()))));
}

#[test]
fn huge_ignore_file_event_is_not_cached_and_has_no_checks() {
    let host = MockHost::install();
    let text = "a\n".repeat(70_000);
    host.add_file(".gitignore", &text);
    let mut plugin = IgnorePlugin::default();
    plugin.handle_events(&[Event::FileOpened("/project/.gitignore".into())], &host.invocation());
    assert!(plugin.cached_checks.is_empty());
    assert!(provider_lines(&host, &mut plugin, "/project/.gitignore", &text).is_empty());
}

#[test]
fn hgignore_same_text_different_syntax_not_duplicate() {
    let problems = check::check("syntax: glob\nfoo\nsyntax: regexp\nfoo\nfoo\n", HG);
    let duplicates: Vec<usize> = problems.iter().filter(|p| p.code == "duplicate-entry").map(|p| p.start).collect();
    assert_eq!(duplicates.len(), 1);
    assert!(duplicates[0] > "syntax: glob\nfoo\nsyntax: regexp\nfoo\n".len() - 1);
}

#[test]
fn completion_finds_late_top_level_dir() {
    let host = MockHost::install();
    host.set_project_root(Some("/project"));
    host.add_file("/project/.gitignore", "");
    for i in 0..2500 {
        host.add_file(&format!("/project/a{i:04}/f.txt"), "");
    }
    host.add_file("/project/zdir/x.js", "");
    host.add_file("/project/zfile.md", "");
    let mut plugin = IgnorePlugin::default();

    let labels: Vec<String> =
        plugin.completion(&doc("/project/.gitignore", "z"), Position::new(0, 1), None).into_iter().map(|i| i.label).collect();

    assert_eq!(labels, ["zdir/", "zfile.md"]);
}

#[test]
fn providers_survive_every_offset() {
    let seeds = ["{{ x }}{#}", "café", "<?php\nuse $😀", "é::class", "<!-- Paramètres --><a/>", "!café/\n日本*\n/😀/x", "syntax: re\né\\"];
    let host = MockHost::install();
    host.set_project_root(Some("/project"));
    host.add_file("/project/café/a.txt", "");
    for path in ["/project/.gitignore", "/project/.hgignore", "/project/.dockerignore"] {
        for seed in seeds {
            let document = doc(path, seed);
            host.open_editor(path, "ignore", seed, &[]);
            let mut plugin = IgnorePlugin::default();
            let mut boundaries: Vec<usize> = seed.char_indices().map(|(i, _)| i).collect();
            boundaries.push(seed.len());
            for offset in boundaries {
                let at = text::position(seed, offset);
                plugin.completion(&document, at, None);
                plugin.definition(&document, at);
                plugin.code_actions(&document, Range::new(at, at));
                plugin.file_actions(&document, Range::new(at, at));
            }
            plugin.line_markers(&document);
            openidea_plugin::semantic_tokens::SemanticTokens::semantic_tokens(&mut plugin, &document);
            openidea_plugin::diagnostics::DiagnosticsProvider::diagnostics(&mut plugin, &document);
        }
    }
}

#[test]
fn offers_add_to_gitignore_for_project_files() {
    let host = MockHost::install();
    host.add_file(".gitignore", "/target/\n/out\\[1\\].log");
    let mut plugin = IgnorePlugin::default();
    let actions = add_actions(&mut plugin,&host,&doc("/project/out[1].log", ""));
    assert_eq!(actions.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(), ["Add !/out\\[1\\].log to .gitignore (unignore)"]);

    let actions = add_actions(&mut plugin,&host,&doc("/project/src/main.rs", ""));
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
fn ignored_status_menu_goes_to_the_rule() {
    let host = MockHost::install();
    host.add_file(".gitignore", "target/\n*.log\n!keep.log\n");
    host.add_file("debug.log", "");
    IgnorePlugin::default().handle_events(&[Event::EditorFocused(Some("/project/debug.log".into()))], &host.invocation());
    assert_eq!(
        host.status_menu(IGNORED_STATUS),
        [
            StatusMenuEntry::Header("Ignored by *.log in .gitignore".into()),
            StatusMenuEntry::action("Go to Rule", "open-ignoring-rule").with_description(".gitignore"),
            StatusMenuEntry::action("Dialect Checks and Preferences", "preferences"),
        ]
    );

    host.open_editor("/project/debug.log", "plaintext", "", &[]);
    IgnorePlugin::default().run_command("open-ignoring-rule", &host.invocation(), None).unwrap();
    assert_eq!(host.opened_locations().last(), Some(&("/project/.gitignore".to_owned(), 1, 0)));

    host.open_editor("/project/keep.log", "plaintext", "", &[]);
    IgnorePlugin::default().run_command("open-ignoring-rule", &host.invocation(), None).unwrap();
    assert!(host.notifications().last().unwrap().body.contains("ignores keep.log"));
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
    host.answer_next_prompt(Some(&["PHP"]));
    plugin.handle("templates", ViewEvent::Button("new".into()), &ctx).unwrap();
    assert!(matches!(plugin.render("templates",None,&ctx).unwrap().body,Body::Tree(_)),"creation keeps the list visible");
    assert_eq!(host.storage().get("template:PHP").map(String::as_str), Some("/vendor/\n"));

    let View { body: Body::Tree(items), .. } = plugin.render("templates", None, &ctx).unwrap() else { panic!() };
    let user = items.iter().find(|i| i.key == "u:PHP").unwrap();
    assert_eq!(user.parent.as_deref(), Some("g:user"));

    plugin.handle("templates", ViewEvent::Activate("u:PHP".into()), &ctx).unwrap();
    assert_eq!(host.editor_text(), "/vendor/\n\n### PHP template\n/vendor/\n");

    plugin.handle("templates", ViewEvent::Select("u:PHP".into()), &ctx).unwrap();
    host.answer_next_prompt(Some(&["ok"]));
    plugin.handle("templates", ViewEvent::Button("delete".into()), &ctx).unwrap();
    assert!(matches!(plugin.render("templates", None, &ctx).unwrap().body, Body::Tree(_)), "deletion never replaces the list with a confirmation page");
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

    host.answer_next_prompt(Some(&["ok"]));
    plugin.handle("templates", ViewEvent::Button("delete".into()), &ctx).unwrap();

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
    host.answer_next_prompt(None);
    plugin.handle("templates", ViewEvent::Button("delete".into()), &ctx).unwrap();
    plugin.handle("templates", ViewEvent::Button("confirm-delete".into()), &ctx).unwrap(); // old events cannot bypass the modal
    assert!(matches!(plugin.render("templates", None, &ctx).unwrap().body, Body::Tree(_)));
    assert_eq!(host.storage().len(), 1);
}

#[test]
fn refuses_to_overwrite_a_user_template() {
    let host = MockHost::install();
    host::storage::set("template:PHP", "old\n").unwrap();
    host.open_editor("/project/.gitignore", "ignore", "new\n", &[(0, 0, 0, 0)]);
    let mut plugin = IgnorePlugin::default();
    host.answer_next_prompt(Some(&["PHP"]));
    let err = plugin.handle("templates", ViewEvent::Button("new".into()), &host.invocation()).unwrap_err();
    assert_eq!(err.to_string(), "A user template named PHP already exists.");
    assert_eq!(host.storage()["template:PHP"], "old\n");
}

#[test]
fn native_template_name_dialog_cancel_and_empty_name_write_nothing() {
    let host=MockHost::install();host.open_editor("/project/.gitignore","ignore","/vendor/\n",&[(0,0,0,0)]);
    let mut plugin=IgnorePlugin::default();let ctx=host.invocation();
    host.answer_next_prompt(None);
    plugin.handle("templates",ViewEvent::Button("new".into()),&ctx).unwrap();
    plugin.handle("templates",ViewEvent::Field("name".into(),"bypass".into()),&ctx).unwrap();
    plugin.handle("templates",ViewEvent::Button("save".into()),&ctx).unwrap();
    assert!(host.storage().is_empty());assert_eq!(host.editor_text(),"/vendor/\n");
    assert!(matches!(plugin.render("templates",None,&ctx).unwrap().body,Body::Tree(_)));
    host.answer_next_prompt(Some(&["   "]));
    assert!(plugin.handle("templates",ViewEvent::Button("new".into()),&ctx).is_err());
    assert!(host.storage().is_empty());assert!(host.notifications().is_empty());
}

#[test]
fn toolbar_buttons_follow_the_editor_and_selection() {
    let host = MockHost::install();
    host::storage::set("template:PHP", "/vendor/\n").unwrap();
    let mut plugin = IgnorePlugin::default();
    let ctx = host.invocation();
    let actions = |plugin: &mut IgnorePlugin| -> Vec<String> {
        plugin.render("templates", None, &ctx).unwrap().toolbar.iter().map(|b| b.id.clone()).collect()
    };
    assert_eq!(actions(&mut plugin), ["preferences"]);

    host.open_editor("/project/.gitignore", "ignore", "", &[(0, 0, 0, 0)]);
    plugin.handle("templates", ViewEvent::Select("t:Rust".into()), &ctx).unwrap();
    assert_eq!(actions(&mut plugin), ["new", "star", "preferences"]);
    plugin.handle("templates", ViewEvent::Select("u:PHP".into()), &ctx).unwrap();
    assert_eq!(actions(&mut plugin), ["new", "delete", "star", "preferences"]);
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
fn user_templates_keep_their_parent_and_only_favorites_use_stars() {
    let items=template_tree(&[".gitignore".into(),"api.php".into()],&HashSet::from(["g:user".into()]),&HashSet::from(["u:api.php".into()]));
    let first=items.iter().find(|item|item.key == "u:.gitignore").unwrap();
    assert_eq!(first.parent.as_deref(),Some("g:user"));assert_eq!(first.icon.as_deref(),Some("file-ignored"));
    let favorite=items.iter().find(|item|item.key == "u:api.php").unwrap();
    assert_eq!(favorite.parent.as_deref(),Some("g:user"));assert_eq!(favorite.icon.as_deref(),Some("star"));
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
    let sections:Vec<_>=manifest.split("[[contributes.fileTemplates]]").skip(1).collect();
    assert_eq!(sections.len(),IGNORE_FILE_NAMES.len());
    for name in IGNORE_FILE_NAMES {
        assert!(sections.iter().any(|section|section.contains(&format!("fileName = \"{name}\""))));
        assert!(dialect(&format!("/project/{name}")).is_some(),"{name}");
    }
    assert!(!manifest.contains("extension = \"ignore\""));
}

fn tree(plugin: &mut IgnorePlugin, host: &MockHost) -> Vec<TreeItem> {
    let View { body: Body::Tree(items), .. } = plugin.render("templates", None, &host.invocation()).unwrap() else { panic!() };
    items
}

fn unignore_edit(content: &str) -> Vec<FileAction> {
    let host = MockHost::install();
    host.add_file(".gitignore", content);
    add_actions(&mut IgnorePlugin::default(),&host,&doc("/project/src/a.rs", ""))
}

#[test]
fn unignore_action_is_absent_when_the_setting_is_off() {
    let host = MockHost::install();
    host.add_file(".gitignore", "/target/\n");
    host.set_setting("unignoreActions", "false");
    let actions = add_actions(&mut IgnorePlugin::default(),&host,&doc("/project/src/a.rs", ""));
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
    let label = |plugin: &mut IgnorePlugin| plugin.render("templates", None, &ctx).unwrap().toolbar.into_iter().find(|b| b.id == "star").unwrap().label;
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
fn the_star_button_is_hidden_without_a_template_selected() {
    let host = MockHost::install();
    let mut plugin = IgnorePlugin::default();
    let ctx = host.invocation();
    plugin.handle("templates", ViewEvent::Select("g:Global".into()), &ctx).unwrap();
    assert!(!plugin.render("templates", None, &ctx).unwrap().toolbar.iter().any(|b| b.id == "star"));
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
    assert!(add_actions(&mut IgnorePlugin::default(),&host,&doc("/project/src/a.rs", "")).is_empty());
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
    let host = new_file_host();
    host.open_editor("/project/.dockerignore", "ignore", "/dist\n", &[(0, 0, 0, 0)]);
    host.add_file("/project/.gitignore", "/out");
    host.select_paths(&["/project/build"]);
    host.answer_next_prompt(Some(&["/project/.gitignore"]));

    IgnorePlugin::default().run_command("add-selected", &host.invocation(), None).unwrap();

    assert_eq!(host.invocation().project().unwrap().read_file("/project/.gitignore").unwrap(), "/out\n/build\n");
    assert_eq!(host.opened_locations(), [("/project/.gitignore".to_owned(), 0, 0)]);
    assert_eq!(host.notifications()[0].body, "Added 1 to .gitignore.");
    assert_eq!(host.editor_text(), "/dist\n");
}

#[test]
fn adding_to_a_closed_ignore_file_without_project_write_explains_the_permission() {
    let host = MockHost::install();
    host.add_file("/project/.gitignore", "");
    host.select_paths(&["/project/build"]);

    let error = IgnorePlugin::default().run_command("add-selected", &host.invocation(), None).unwrap_err().to_string();

    assert!(error.contains("project.write permission"), "{error}");
    assert_eq!(host.invocation().project().unwrap().read_file("/project/.gitignore").unwrap(), "");
}

#[test]
fn adding_without_a_selection_asks_for_one() {
    let host = new_file_host();
    host.add_file("/project/.gitignore", "");

    let error = IgnorePlugin::default().run_command("add-selected", &host.invocation(), None).unwrap_err().to_string();

    assert!(error.contains("Select files or folders"), "{error}");
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

#[test]
fn unignore_selected_adds_negated_entries() {
    let host = MockHost::install();
    host.open_editor("/project/.gitignore", "ignore", "/dist\n", &[(0, 0, 0, 0)]);
    host.select_paths(&["/project/dist/keep.txt"]);

    IgnorePlugin::default().run_command("unignore-selected", &host.invocation(), None).unwrap();

    assert_eq!(host.editor_text(), "/dist\n!/dist/keep.txt\n");
    assert_eq!(host.notifications().len(), 1);
}

#[test]
fn the_editor_menu_adds_the_current_file_when_nothing_is_selected() {
    let host = new_file_host();
    host.open_editor("/project/src/a.rs", "rust", "", &[(0, 0, 0, 0)]);
    host.add_file("/project/.gitignore", "");
    host.answer_next_prompt(Some(&["/project/.gitignore"]));

    IgnorePlugin::default().run_command("add-selected", &host.invocation(), None).unwrap();

    assert_eq!(host.invocation().project().unwrap().read_file("/project/.gitignore").unwrap(), "/src/a.rs\n");
    assert_eq!(host.opened_locations(), [("/project/.gitignore".to_owned(), 0, 0)]);
}

fn new_file_host() -> MockHost {
    MockHost::install().with_capabilities(&["project.read", "project.write", "editor.read", "editor.write", "storage"])
}

#[test]
fn new_ignore_file_command_creates_from_editor() {
    let host = new_file_host();
    host.add_file("src/a.rs", "");
    host.open_editor("/project/src/a.rs", "rust", "", &[]);
    host.answer_next_prompt(Some(&[".dockerignore"]));
    host.answer_next_prompt(Some(&[""]));
    host.answer_next_prompt(Some(&[]));

    IgnorePlugin::default().run_command("new-file", &host.invocation(), None).unwrap();

    assert_eq!(host.invocation().project().unwrap().read_file(".dockerignore").unwrap(), "# .dockerignore\n");
    assert_eq!(host.opened_locations(), [("/project/.dockerignore".to_owned(), 0, 0)]);
    assert!(host.process_runs().is_empty());
}

#[test]
fn new_ignore_file_without_editor_picks_type() {
    let host = new_file_host();
    host.answer_next_prompt(Some(&[".npmignore"]));
    host.answer_next_prompt(Some(&[]));

    IgnorePlugin::default().run_command("new-file", &host.invocation(), None).unwrap();

    assert_eq!(host.invocation().project().unwrap().read_file(".npmignore").unwrap(), "");
    assert_eq!(host.opened_locations(), [("/project/.npmignore".to_owned(), 0, 0)]);
}

#[test]
fn new_ignore_file_goes_in_the_selected_folder() {
    let host = new_file_host();
    host.add_file("web/src/a.rs", "");
    host.open_editor("/project/web/src/a.rs", "rust", "", &[]);
    host.add_dir("web");
    host.select_paths(&["/project/web"]);
    host.answer_next_prompt(Some(&[".npmignore"]));
    host.answer_next_prompt(Some(&[]));

    IgnorePlugin::default().run_command("new-file", &host.invocation(), None).unwrap();

    assert_eq!(host.invocation().project().unwrap().read_file("web/.npmignore").unwrap(), "");
}

#[test]
fn new_ignore_file_next_to_a_selected_file_goes_in_its_folder() {
    let host = new_file_host();
    host.add_file("web/package.json", "{}");
    host.select_paths(&["/project/web/package.json"]);
    host.answer_next_prompt(Some(&[".npmignore"]));
    host.answer_next_prompt(Some(&[]));

    IgnorePlugin::default().run_command("new-file", &host.invocation(), None).unwrap();

    assert_eq!(host.invocation().project().unwrap().read_file("web/.npmignore").unwrap(), "");
}

#[test]
fn new_ignore_file_refuses_existing() {
    let host = new_file_host();
    host.add_file(".gitignore", "old\n");
    host.answer_next_prompt(Some(&[".gitignore"]));

    let error = IgnorePlugin::default().run_command("new-file", &host.invocation(), None).unwrap_err().to_string();

    assert!(error.contains("already exists"), "{error}");
    assert_eq!(host.invocation().project().unwrap().read_file(".gitignore").unwrap(), "old\n");
}

#[test]
fn shared_context_actions_use_the_clicked_template_and_reject_builtin_deletion() {
    let host = MockHost::install();
    host::storage::set("template:Mine", "/vendor/\n").unwrap();
    let ctx=host.invocation();let mut plugin=IgnorePlugin::default();
    plugin.handle("templates",ViewEvent::Select("t:Rust".into()),&ctx).unwrap();
    let root=ctx.project().unwrap().root().unwrap();
    let action=|plugin:&mut IgnorePlugin,key:&str,button:&str| {
        let request=serde_json::json!({"channel":"ui-kit","sequence":1,"op":"item-button","project":root,"key":key,"button":button});
        plugin.handle("templates",ViewEvent::Message(request.to_string()),&ctx).unwrap();
        serde_json::from_str::<serde_json::Value>(&host.view_messages().last().unwrap().1).unwrap()
    };
    assert!(action(&mut plugin,"u:Mine","star").get("error").is_none());
    assert_eq!(host.storage().get("starred:u:Mine").map(String::as_str),Some("1"));
    assert!(!host.storage().contains_key("starred:t:Rust"));
    assert!(action(&mut plugin,"s:u:Mine","star").get("error").is_none());
    assert!(!host.storage().contains_key("starred:u:Mine"));
    assert!(action(&mut plugin,"t:Rust","delete").get("error").is_some());
    assert!(host.storage().contains_key("template:Mine"));
}

#[test]
fn packaged_template_activation_describes_insertion_and_requires_an_owned_glob_editor() {
    let host=MockHost::install();let mut plugin=IgnorePlugin::default();
    let model=|plugin:&mut IgnorePlugin| {
        plugin.handle("templates",ViewEvent::Message(serde_json::json!({"channel":"ui-kit","sequence":1,"op":"render"}).to_string()),&host.invocation()).unwrap();
        serde_json::from_str::<serde_json::Value>(&host.view_messages().last().unwrap().1).unwrap()["model"].clone()
    };
    let activation=|model:&serde_json::Value|model["body"]["items"].as_array().unwrap().iter().find(|item|item["key"]=="t:Rust").unwrap().clone();
    let no_editor=model(&mut plugin);let row=activation(&no_editor);
    assert_eq!(row["activationLabel"],"Add to Ignore File");assert_eq!(row["activationEnabled"],false);
    assert_eq!(no_editor["contextButtons"],serde_json::json!(["new","delete","star"]));
    host.open_editor("/project/.gitignore","ignore","",&[]);assert_eq!(activation(&model(&mut plugin))["activationEnabled"],true);
    host.open_editor("/project/source.rs","rust","",&[]);assert_eq!(activation(&model(&mut plugin))["activationEnabled"],false);
    assert_eq!(host.editor_text(), "");
}

#[test]
fn pattern_inspections_preserve_significant_gitignore_spaces() {
    let dialect = crate::check::dialect(".gitignore").unwrap();
    let checks = crate::check::check("name\n name\nname\\ \n", dialect);
    assert!(!checks.iter().any(|p| p.code == "duplicate-entry" || p.code == "cover-entry"), "{checks:?}");
    let checks = crate::check::check("name   \nname\n", dialect);
    assert!(checks.iter().any(|p| p.code == "duplicate-entry"));
}

#[test]
fn covered_entries_are_found_below_the_cap_and_skipped_above_it() {
    let entries = |n: usize| (0..n).map(|i| format!("dir{i}/file{i}.txt\n")).collect::<String>();
    let small = format!("build/\n{}build/out.o\n", entries(400));
    assert!(check(&small, GIT).iter().any(|p| p.code == "cover-entry"));
    let large = format!("build/\n{}build/out.o\n", entries(1000));
    let started = std::time::Instant::now();
    let problems = check(&large, GIT);
    assert!(started.elapsed() < std::time::Duration::from_millis(200), "{:?}", started.elapsed());
    assert!(!problems.iter().any(|p| p.code == "cover-entry"), "above 512 entries redundancy is not checked");
}

#[test]
fn code_actions_reuse_the_checked_problems() {
    let _host = MockHost::install();
    let mut plugin = IgnorePlugin::default();
    let text = "a\nb\na\n";
    let document = doc("/project/.gitignore", text);
    openidea_plugin::diagnostics::DiagnosticsProvider::diagnostics(&mut plugin, &document);
    assert_eq!(plugin.cached_checks.len(), 1);
    plugin.cached_checks[0].2.clear();
    let line = Range::new(Position::new(2, 0), Position::new(2, 0));
    assert!(plugin.code_actions(&document, line).iter().all(|a| a.title != "Remove entry"), "served from the cache");
}
