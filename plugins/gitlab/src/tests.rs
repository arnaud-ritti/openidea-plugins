use super::*;
use openidea_plugin::mock::MockHost;
use openidea_plugin::{HttpRequest, HttpResponse, ProcessOutput, StatusMenuEntry};

fn forwarded_message(message:&str)->String {
    let value=serde_json::from_str::<serde_json::Value>(message).unwrap_or_default();
    value["message"].as_str().unwrap_or(message).to_owned()
}

const CAPS: &[&str] =
    &["vcs.read", "editor.read", "net:gitlab.com", "secrets", "storage", "clipboard", "project.read", "events", "process:git"];

const MR: &str = r#"{
    "iid": 7, "title": "Add parser", "description": "Adds it", "state": "opened", "draft": false,
    "source_project_id": 5, "target_project_id": 5,
    "diff_refs": {"base_sha": "abcdef0", "head_sha": "abcdef1", "start_sha": "start"},
    "source_branch": "feature", "target_branch": "main", "author": {"id": 2, "username": "ann", "name": "Ann"},
    "reviewers": [], "web_url": "https://gitlab.com/g/p/-/merge_requests/7", "sha": "abc123",
    "detailed_merge_status": "mergeable", "head_pipeline": {"status": "success"},
    "force_remove_source_branch": true, "squash": false, "user": {"can_merge": true}
}"#;

const DISCUSSIONS: &str = r#"[
    {"id": "d1", "notes": [
        {"id": 11, "body": "Why?", "author": {"username": "bob"}, "created_at": "2026-01-02", "resolvable": true, "resolved": false,
         "position": {"new_path": "src/a.rs", "new_line": 4}},
        {"id": 12, "body": "Because", "author": {"username": "ann"}, "created_at": "2026-01-03"}
    ]},
    {"id": "d2", "notes": [{"id": 13, "body": "changed the description", "author": {"username": "ann"}, "created_at": "2026-01-01", "system": true}]}
]"#;

const DIFFS: &str = r#"[{"old_path": "src/a.rs", "new_path": "src/a.rs", "diff": "@@ -3,2 +3,2 @@\n a\n-b\n+c\n x\n"}]"#;

fn ok(body: &str) -> Result<HttpResponse, Error> {
    Ok(HttpResponse { status: 200, headers: vec![], body: body.as_bytes().to_vec() })
}

fn gitlab(req: &HttpRequest) -> Result<HttpResponse, Error> {
    let path = req.url.strip_prefix("https://gitlab.com").unwrap();
    let mr = "/api/v4/projects/g%2Fp/merge_requests/7";

    match (req.method.as_str(), path) {
        ("GET", "/api/v4/user") => ok(r#"{"id": 1, "username": "me", "name": "Me"}"#),
        ("GET", "/api/v4/projects/g%2Fp") => ok(r#"{"id": 5, "default_branch": "main"}"#),
        ("GET", p) if p.starts_with("/api/v4/projects?membership=true") => {
            ok(r#"[{"name": "Tool", "path": "tool", "path_with_namespace": "me/tool", "name_with_namespace": "Me / Tool",
                "http_url_to_repo": "https://gitlab.com/me/tool.git", "ssh_url_to_repo": "git@gitlab.com:me/tool.git"}]"#)
        }
        ("GET", p) if p.starts_with("/api/v4/projects/g%2Fp/merge_requests?") => ok(&format!("[{MR}]")),
        ("GET", p) if p.split('?').next() == Some(mr) => ok(MR),
        ("GET", p) if p == format!("{mr}/approvals") => {
            ok(r#"{"approved_by": [{"user": {"username": "bob"}}], "approvals_left": 1, "user_can_approve": true}"#)
        }
        ("GET", p) if p == format!("{mr}/draft_notes") => ok("[]"),
        ("GET", p) if p.starts_with(&format!("{mr}/discussions")) => ok(DISCUSSIONS),
        ("GET", p) if p.starts_with(&format!("{mr}/diffs")) => ok(DIFFS),
        ("GET", p) if p.starts_with(&format!("{mr}/resource_state_events")) => {
            ok(r#"[{"created_at": "2026-01-04", "user": {"username": "bob"}, "state": "reopened"}]"#)
        }
        ("GET", p) if p.starts_with(&format!("{mr}/resource_")) => ok("[]"),
        ("GET", p) if p.starts_with("/api/v4/projects/g%2Fp/repository/branches?") => {
            ok(r#"[{"name": "main"}, {"name": "develop"}, {"name": "feature"}]"#)
        }
        ("GET", p) if p.starts_with("/api/v4/projects/g%2Fp/labels") => ok(r#"[{"name": "bug"}, {"name": "ui"}]"#),
        ("GET", p) if p.starts_with("/api/v4/projects/g%2Fp/members/all") => {
            ok(r#"[{"id": 2, "username": "ann", "name": "Ann"}, {"id": 3, "username": "bob", "name": "Bob"}]"#)
        }
        ("GET", p) if p.starts_with("/api/v4/projects/g%2Fp/milestones") => ok(r#"[{"id": 40, "title": "v2"}]"#),
        ("GET", p) if p.starts_with("/api/v4/namespaces?") => {
            ok(r#"[{"id": 3, "full_path": "me", "kind": "user"}, {"id": 12, "full_path": "team/sub", "kind": "group"}]"#)
        }
        ("GET", "/api/v4/users?username=ann") => ok(r#"[{"id": 2, "username": "ann"}]"#),
        ("GET", "/api/v4/users?username=bob") => ok(r#"[{"id": 3, "username": "bob"}]"#),
        ("GET", p) if p.starts_with("/api/v4/users?") => ok("[]"),
        ("POST", "/api/v4/projects/g%2Fp/merge_requests") => {
            ok(r#"{"iid": 8, "title": "Add parser", "web_url": "https://gitlab.com/g/p/-/merge_requests/8"}"#)
        }
        ("POST", "/api/v4/snippets") => ok(r#"{"web_url": "https://gitlab.com/-/snippets/9"}"#),
        ("POST", "/api/v4/projects/g%2Fp/snippets") => ok(r#"{"web_url": "https://gitlab.com/g/p/-/snippets/10"}"#),
        ("POST", "/api/v4/projects") => {
            ok(r#"{"web_url": "https://gitlab.com/me/project", "http_url_to_repo": "https://gitlab.com/me/project.git",
                "ssh_url_to_repo": "git@gitlab.com:me/project.git"}"#)
        }
        ("POST" | "PUT", _) => ok("{}"),
        _ => Ok(HttpResponse { status: 404, headers: vec![], body: br#"{"message":"404 Not Found"}"#.to_vec() }),
    }
}

fn status(branch: &str) -> RepositoryStatus {
    RepositoryStatus {
        root: "/project".into(),
        branch: Some(branch.into()),
        commit: Some("abc123".into()),
        upstream: Some(format!("origin/{branch}")),
        ..RepositoryStatus::default()
    }
}

fn setup() -> MockHost {
    let host = MockHost::install().with_capabilities(CAPS);
    host.set_secret("token", "glpat-x");
    host.set_vcs_status(Some(status("main")));
    host.set_vcs_remotes(&[("upstream", "git@github.com:x/y.git"), ("origin", "git@gitlab.com:g/p.git")]);
    host.on_fetch(gitlab);
    host
}

fn render_in(plugin: &mut GitLab, section: Section, host: &MockHost) -> View {
    storage::set(sections::SECTION_KEY, section.id()).unwrap();

    plugin.render(DETAILS, None, &host.invocation()).unwrap()
}

fn handle_in(plugin: &mut GitLab, section: Section, event: ViewEvent, host: &MockHost) {
    storage::set(sections::SECTION_KEY, section.id()).unwrap();
    let event=if section==Section::Files {
        match event { ViewEvent::Button(kind) if matches!(kind.as_str(),"clear"|"mark-all") => {
            let label=if kind=="clear" {"Clear Viewed"} else {"Mark All Viewed"};
            let view=render_in(plugin,section,host);ViewEvent::Button(view.toolbar.iter().find(|button|button.label==label).unwrap().id.clone())
        }, other=>other }
    } else if section==Section::Pipelines {
        match event { ViewEvent::Button(kind) if matches!(kind.as_str(),"retry"|"cancel"|"browser"|"open-job-trace-snapshot") => {
            let view=render_in(plugin,section,host);
            let key=view.toolbar.iter().find(|button|button.id.strip_prefix("pipeline-action:").and_then(|json|serde_json::from_str::<serde_json::Value>(json).ok()).is_some_and(|value|value[4].as_str()==Some(&kind)))
                .map(|button|button.id.clone()).unwrap_or(kind);ViewEvent::Button(key)
        },other=>other }
    } else {event};
    plugin.handle(DETAILS, event, &host.invocation()).unwrap();
}

fn selected(host: &MockHost) -> GitLab {
    let mut plugin = GitLab::default();
    plugin.handle(LIST, ViewEvent::Select("mr:7".into()), &host.invocation()).unwrap();
    plugin.render(DETAILS, None, &host.invocation()).unwrap();
    plugin
}

fn changes_view(plugin: &mut GitLab, host: &MockHost) -> Result<View, Error> {
    storage::set(sections::SECTION_KEY, "changes").unwrap();
    plugin.render(DETAILS, None, &host.invocation())
}

fn changes_message(plugin: &mut GitLab, host: &MockHost, event: ViewEvent) -> Result<(), Error> {
    storage::set(sections::SECTION_KEY, "changes").unwrap();
    let event = if let ViewEvent::Message(message) = event {
        let mut value: serde_json::Value=serde_json::from_str(&message).unwrap();
        if value.get("reviewContext").is_none() {
            if plugin.review_context(&host.invocation()).is_err() { changes_view(plugin,host)?; }
            value["reviewContext"]=serde_json::Value::String(plugin.review_context(&host.invocation())?);
        }
        ViewEvent::Message(value.to_string())
    } else { event };
    plugin.handle(DETAILS, event, &host.invocation())
}

fn section_buttons(view: &View) -> Vec<(&str, &str)> {
    view.toolbar.iter().filter(|b| b.id.starts_with("section:")).map(|b| (b.id.as_str(), b.label.as_str())).collect()
}

fn field(key: &str, value: &str) -> ViewEvent {
    ViewEvent::Field(key.into(), value.into())
}

fn button_matches(key: &str, kind: &str) -> bool {
    key==kind || key.strip_prefix("pipeline-action:").and_then(|json|serde_json::from_str::<serde_json::Value>(json).ok()).is_some_and(|value|value[4].as_str()==Some(kind))
}
fn label<'a>(view: &'a View, id: &str) -> &'a str {
    &view.toolbar.iter().find(|b| button_matches(&b.id,id)).unwrap().label
}

fn buttons(view: &View) -> Vec<&str> {
    view.toolbar.iter().map(|b| b.id.as_str()).collect()
}

fn writes(host: &MockHost) -> Vec<(String, String, String)> {
    host.requests()
        .into_iter()
        .filter(|r| r.method != "GET")
        .map(|r| (r.method, r.url, String::from_utf8(r.body.unwrap_or_default()).unwrap()))
        .collect()
}

fn git_ok(_: &[String], _: Option<&str>) -> Result<ProcessOutput, Error> {
    Ok(ProcessOutput { status: Some(0), stdout: String::new(), stderr: String::new() })
}

fn git_runs(host: &MockHost) -> Vec<String> {
    host.process_runs().into_iter().map(|r| r.args.join(" ")).collect()
}

#[test]
fn without_a_token_the_views_ask_for_one() {
    let host = MockHost::install().with_capabilities(CAPS);
    let mut plugin = GitLab::default();

    let view = plugin.render(LIST, None, &host.invocation()).unwrap();

    assert_eq!(view.body, Body::Message(NO_TOKEN.into()));
    assert!(host.requests().is_empty());
}

#[test]
fn opening_a_listed_merge_request_selects_custom_details_and_keeps_browser_access() {
    let host=setup();let mut plugin=GitLab::default();
    plugin.render(LIST,None,&host.invocation()).unwrap();
    plugin.handle(LIST,ViewEvent::Activate("mr:7".into()),&host.invocation()).unwrap();
    assert_eq!(host.revealed_views(),[LIST]);assert_eq!(GitLab::selected(),Some(7));
    assert_eq!(ui_kit::web::active_view(LIST,include_str!("ui_config.json")).unwrap(),DETAILS);
    assert!(host.opened_urls().is_empty());
    let requests=host.requests().len();
    assert!(plugin.handle(LIST,ViewEvent::Activate("mr:999".into()),&host.invocation()).is_err());
    assert_eq!(host.revealed_views(),[LIST]);assert_eq!(host.requests().len(),requests);
    plugin.render(DETAILS,None,&host.invocation()).unwrap();
    plugin.handle(DETAILS,ViewEvent::Button("browser".into()),&host.invocation()).unwrap();
    assert_eq!(host.opened_urls(),["https://gitlab.com/g/p/-/merge_requests/7"]);
}

#[test]
fn lists_merge_requests_assigned_to_me_then_cycles_filters() {
    let host = setup();
    let mut plugin = GitLab::default();

    let view = plugin.render(LIST, None, &host.invocation()).unwrap();

    let Body::Tree(items) = &view.body else { panic!("{view:?}") };
    assert_eq!(items[0].label, "!7 Add parser");
    assert_eq!(items[0].icon.as_deref(), Some("github-pr-open"));
    assert_eq!(items[0].detail.as_deref(), Some("ann · feature → main"));
    let list = host.requests().into_iter().find(|r| r.url.contains("merge_requests?")).unwrap();
    assert!(list.url.contains("state=opened&scope=assigned_to_me"));
    assert_eq!(list.headers[0], ("Authorization".into(), "Bearer glpat-x".into()));

    host.answer_next_prompt(Some(&["quick"]));
    host.answer_next_prompt(Some(&["closed"]));
    plugin.handle(LIST, ViewEvent::Button("filter".into()), &host.invocation()).unwrap();
    plugin.render(LIST, None, &host.invocation()).unwrap();

    assert!(host.requests().last().unwrap().url.contains("state=closed&scope=all"));
}

#[test]
fn the_timeline_interleaves_threads_and_events_until_hidden() {
    let host = setup();
    let mut plugin = GitLab::default();
    plugin.render(LIST, None, &host.invocation()).unwrap();

    plugin.handle(LIST, ViewEvent::Expand("mr:7".into()), &host.invocation()).unwrap();
    let view = plugin.render(LIST, None, &host.invocation()).unwrap();

    let Body::Tree(items) = &view.body else { panic!("{view:?}") };
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["!7 Add parser", "ann changed the description", "Why?", "Because", "bob reopened"]);
    assert_eq!(items[2].detail.as_deref(), Some("bob · 1 reply · unresolved · src/a.rs:4"));
    let icons: Vec<&str> = items.iter().filter_map(|i| i.icon.as_deref()).collect();
    assert_eq!(icons, ["github-pr-open", "history", "review-comment", "review-comment", "history"]);
    assert_eq!(items[4].detail.as_deref(), Some("2026-01-04"));

    plugin.handle(LIST, ViewEvent::Button("events".into()), &host.invocation()).unwrap();
    let view = plugin.render(LIST, None, &host.invocation()).unwrap();

    let Body::Tree(items) = &view.body else { panic!("{view:?}") };
    let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys, ["mr:7", "d:7:d1", "n:7:d1:12"]);
}

#[test]
fn a_thread_with_replies_opens_and_closes() {
    let host = setup();
    let mut plugin = GitLab::default();
    plugin.render(LIST, None, &host.invocation()).unwrap();
    plugin.handle(LIST, ViewEvent::Expand("mr:7".into()), &host.invocation()).unwrap();
    let thread = |plugin: &mut GitLab| {
        let view = plugin.render(LIST, None, &host.invocation()).unwrap();
        let Body::Tree(items) = view.body else { panic!("{view:?}") };
        items.into_iter().find(|i| i.key == "d:7:d1").unwrap()
    };
    assert!(thread(&mut plugin).expandable && !thread(&mut plugin).expanded);

    plugin.handle(LIST, ViewEvent::Expand("d:7:d1".into()), &host.invocation()).unwrap();
    assert!(thread(&mut plugin).expanded);

    plugin.handle(LIST, ViewEvent::Collapse("d:7:d1".into()), &host.invocation()).unwrap();
    assert!(!thread(&mut plugin).expanded);
}

#[test]
fn activating_a_diff_thread_opens_its_line() {
    let host = setup();
    let mut plugin = GitLab::default();
    plugin.render(LIST, None, &host.invocation()).unwrap();
    plugin.handle(LIST, ViewEvent::Expand("mr:7".into()), &host.invocation()).unwrap();

    plugin.handle(LIST, ViewEvent::Activate("d:7:d1".into()), &host.invocation()).unwrap();

    assert_eq!(host.opened_locations(), [("/project/src/a.rs".to_owned(), 3, 0)]);
}

#[test]
fn details_offer_the_review_flow() {
    let host = setup();

    let view = selected(&host).render(DETAILS, None, &host.invocation()).unwrap();

    let actions: Vec<&str> = buttons(&view).into_iter().filter(|b| !b.starts_with("section:")).collect();
    assert_eq!(
        actions,
        [
            "comment",
            "approve",
            "merge",
            "edit",
            "close",
            "checkout",
            "browser",
            "more-actions",
            "refresh"
        ]
    );
    assert!(view.toolbar.iter().filter(|b| !b.id.starts_with("section:")).all(|b| b.icon.is_some()), "{view:?}");
    assert_eq!(
        view.status.as_deref(),
        Some("Open · feature → main · ann · Ready to merge · Pipeline passed · Approved by bob · 1 more required")
    );
    let Body::Form(fields) = &view.body else { panic!("{view:?}") };
    let keys: Vec<&str> = fields.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["comment", "as-draft", "emoji", "remove-source-branch", "squash", "description"]);
    assert_eq!(fields[2].value, FieldValue::Choice(safe::EMOJIS.map(str::to_owned).to_vec(), "thumbsup".into()));
    assert_eq!(fields[5].label, "Description");
    assert_eq!(fields[5].value, FieldValue::readonly_text("Adds it", true));
    assert!(!fields.iter().any(|f| matches!(f.value, FieldValue::Text(_))), "no read-only facts as inputs: {fields:?}");
}

#[test]
fn more_lists_only_available_secondary_actions_and_runs_the_chosen_one() {
    let host = setup();
    let mut plugin = selected(&host);
    plugin.render(DETAILS, None, &host.invocation()).unwrap();

    let offered: Vec<&str> = plugin.more_actions().unwrap().into_iter().map(|(id, _)| id).collect();
    assert_eq!(offered, ["react", "review", "draft", "open-overview-snapshot", "open-diff-snapshot", "open-existing-worktree", "copy"]);

    host.answer_next_prompt(Some(&["rerequest-review"]));
    assert!(plugin.handle(DETAILS, ViewEvent::Button("more-actions".into()), &host.invocation()).is_err(), "no reviewers: not offered");

    host.answer_next_prompt(None);
    plugin.handle(DETAILS, ViewEvent::Button("more-actions".into()), &host.invocation()).unwrap();
    assert!(writes(&host).is_empty(), "a cancelled picker does nothing");

    host.answer_next_prompt(Some(&["draft"]));
    plugin.handle(DETAILS, ViewEvent::Button("more-actions".into()), &host.invocation()).unwrap();
    assert!(writes(&host)[0].2.contains(r#""draft":true"#));
    assert_eq!(plugin.status.as_deref(), Some("Marked as draft."));
}

#[test]
fn a_selected_thread_shows_who_is_replied_to_before_the_reply() {
    let host = setup();
    let mut plugin = GitLab::default();
    plugin.handle(LIST, ViewEvent::Select("d:7:d1".into()), &host.invocation()).unwrap();

    let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();

    let Body::Form(fields) = &view.body else { panic!("{view:?}") };
    assert_eq!(fields[0], Field::new("replying-to", "Replying to", FieldValue::readonly_text("bob: Why?", false)));
    assert_eq!(fields[1].label, "Reply");
}

#[test]
fn edits_to_the_description_are_ignored_and_the_comment_survives_a_redraw() {
    let host = setup();
    let mut plugin = selected(&host);

    plugin.handle(DETAILS, field("description", "changed"), &host.invocation()).unwrap();
    plugin.handle(DETAILS, field("comment", "draft text"), &host.invocation()).unwrap();
    let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();

    let Body::Form(fields) = &view.body else { panic!("{view:?}") };
    assert_eq!(fields[0].value, FieldValue::Multiline("draft text".into()));
    assert_eq!(fields.last().unwrap().value, FieldValue::readonly_text("Adds it", true));
    assert!(writes(&host).is_empty());
}

#[test]
fn a_failed_comment_keeps_its_text_and_says_why() {
    let host = setup();
    host.on_fetch(|r| match r.method == "POST" {
        true => Ok(HttpResponse { status: 400, headers: vec![], body: br#"{"message":"note is too long"}"#.to_vec() }),
        false => gitlab(r),
    });
    let mut plugin = selected(&host);

    plugin.handle(DETAILS, field("comment", "LGTM"), &host.invocation()).unwrap();
    plugin.handle(DETAILS, ViewEvent::Button("comment".into()), &host.invocation()).unwrap();
    let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();

    assert_eq!(host.storage().get("comment").map(String::as_str), Some("LGTM"));
    assert!(view.status.as_deref().unwrap().contains("note is too long"), "{view:?}");
    let Body::Form(fields) = &view.body else { panic!("{view:?}") };
    assert_eq!(fields[0].value, FieldValue::Multiline("LGTM".into()));
}

#[test]
fn merge_request_states_read_as_labels() {
    let mut mr = MergeRequest { state: "merged".into(), ..MergeRequest::default() };
    assert_eq!(state_label(&mr.state), "Merged");
    assert_eq!(merge_request_icon(&mr), "github-pr-merged");

    mr.detailed_merge_status = Some("ci_must_pass".into());
    mr.head_pipeline = serde_json::from_str(r#"{"status": "failed"}"#).ok();
    assert_eq!(merge_status(&mr), "Pipeline must pass · Pipeline failed");

    mr.detailed_merge_status = Some("some_new_check".into());
    mr.head_pipeline = None;
    assert_eq!(merge_status(&mr), "Some new check");

    mr.draft = true;
    assert_eq!(merge_request_icon(&mr), "github-pr-draft");
}

#[test]
fn comments_go_to_the_review_by_default_then_post_directly() {
    let host = setup();
    let mut plugin = selected(&host);

    plugin.handle(DETAILS, field("comment", "LGTM"), &host.invocation()).unwrap();
    plugin.handle(DETAILS, ViewEvent::Button("comment".into()), &host.invocation()).unwrap();
    plugin.handle(DETAILS, field("as-draft", "false"), &host.invocation()).unwrap();
    plugin.handle(DETAILS, field("comment", "Ship it"), &host.invocation()).unwrap();
    plugin.render(DETAILS, None, &host.invocation()).unwrap();
    plugin.handle(DETAILS, ViewEvent::Button("comment".into()), &host.invocation()).unwrap();

    let posted = writes(&host);
    assert_eq!(posted[0].1, "https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7/draft_notes");
    assert_eq!(posted[0].2, r#"{"note":"LGTM"}"#);
    assert_eq!(posted[1].1, "https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7/discussions");
    assert_eq!(posted[1].2, r#"{"body":"Ship it"}"#);
    assert_eq!(plugin.status.as_deref(), Some("Comment posted."));
    assert_eq!(host.storage().get("comment"), None);
}

#[test]
fn submit_review_publishes_drafts_with_the_summary() {
    let host = setup();
    host.on_fetch(|r| match r.url.ends_with("/draft_notes") && r.method == "GET" {
        true => ok(r#"[{"id": 1, "note": "nit"}]"#),
        false => gitlab(r),
    });
    let mut plugin = selected(&host);

    let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();
    plugin.handle(DETAILS, field("comment", "Looks good"), &host.invocation()).unwrap();
    plugin.handle(DETAILS, ViewEvent::Button("submit".into()), &host.invocation()).unwrap();

    assert!(view.toolbar.iter().any(|b| b.label == "Submit Review (1)"));
    let posted = writes(&host);
    assert!(posted[0].1.ends_with("/merge_requests/7/draft_notes/bulk_publish"));
    assert_eq!(posted[1].2, r#"{"body":"Looks good"}"#);
    assert_eq!(plugin.status.as_deref(), Some("Review submitted."));
}

#[test]
fn a_selected_thread_gets_the_reply_and_can_be_resolved() {
    let host = setup();
    let mut plugin = GitLab::default();
    plugin.handle(LIST, ViewEvent::Select("n:7:d1:12".into()), &host.invocation()).unwrap();
    plugin.render(DETAILS, None, &host.invocation()).unwrap();

    plugin.handle(DETAILS, field("as-draft", "false"), &host.invocation()).unwrap();
    plugin.handle(DETAILS, field("comment", "ok"), &host.invocation()).unwrap();
    plugin.handle(DETAILS, ViewEvent::Button("comment".into()), &host.invocation()).unwrap();
    plugin.render(DETAILS, None, &host.invocation()).unwrap();
    plugin.handle(DETAILS, ViewEvent::Button("resolve".into()), &host.invocation()).unwrap();

    let posted = writes(&host);
    assert!(posted[0].1.ends_with("/merge_requests/7/discussions/d1/notes"));
    assert!(posted[1].1.ends_with("/merge_requests/7/discussions/d1?resolved=true"));
}

#[test]
fn merge_sends_the_head_sha_and_options() {
    let host = setup();
    let mut plugin = selected(&host);

    plugin.handle(DETAILS, field("squash", "true"), &host.invocation()).unwrap();
    plugin.handle(DETAILS, ViewEvent::Button("merge".into()), &host.invocation()).unwrap();

    assert!(writes(&host).is_empty(), "the first click only asks to confirm");
    let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();
    assert_eq!(label(&view, "merge"), "Confirm Merge !7 into main (squash, delete source branch)");

    plugin.handle(DETAILS, ViewEvent::Button("merge".into()), &host.invocation()).unwrap();

    let posted = writes(&host);
    assert_eq!(posted[0].0, "PUT");
    assert!(posted[0].1.ends_with("/merge_requests/7/merge"));
    assert_eq!(posted[0].2, r#"{"sha":"abc123","should_remove_source_branch":true,"squash":true}"#);
    assert!(host.refreshed_views().ends_with(&[LIST.into(), LIST.into()]));
}

fn branch_on_merge_request_8() -> MockHost {
    let host = setup();
    host.set_vcs_status(Some(status("feature")));
    host.on_fetch(|r| {
        let mr8 = MR.replace(r#""iid": 7"#, r#""iid": 8"#);
        if r.url.contains("source_branch=feature") {
            return ok(&format!("[{mr8}]"));
        }
        if r.url.split('?').next().unwrap().ends_with("/merge_requests/8") {
            return ok(&mr8);
        }
        let mut as_7 = r.clone();
        as_7.url = r.url.replace("/merge_requests/8/", "/merge_requests/7/");
        gitlab(&as_7)
    });
    host
}

#[test]
fn a_merge_confirm_does_not_follow_a_switch_to_the_branch_merge_request() {
    let host = branch_on_merge_request_8();
    let mut plugin = selected(&host);
    plugin.handle(DETAILS, ViewEvent::Button("merge".into()), &host.invocation()).unwrap();
    plugin.handle_events(&[Event::VcsChanged("/project".into())], &host.invocation());

    plugin.run_command("show-branch-merge-request", &host.invocation(), None).unwrap();
    plugin.render(DETAILS, None, &host.invocation()).unwrap();
    plugin.handle(DETAILS, ViewEvent::Button("merge".into()), &host.invocation()).unwrap();

    assert!(writes(&host).is_empty(), "{:?}", writes(&host));
    let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();
    assert_eq!(label(&view, "merge"), "Confirm Merge !8 into main (delete source branch)");
}

#[test]
fn show_current_branch_merge_request_works_without_project_read() {
    let host = branch_on_merge_request_8().with_capabilities(&["vcs.read", "editor.read", "net:gitlab.com", "secrets", "storage"]);
    let mut plugin = GitLab::default();

    plugin.run_command("show-branch-merge-request", &host.invocation(), None).unwrap();

    assert_eq!(host.storage().get("selected").map(String::as_str), Some("8"));
}

#[test]
fn a_close_confirm_is_bound_to_its_merge_request() {
    let host = branch_on_merge_request_8();
    let mut plugin = selected(&host);
    plugin.handle(DETAILS, ViewEvent::Button("close".into()), &host.invocation()).unwrap();
    storage::set(SELECTED_KEY, "8").unwrap();
    plugin.details = None;
    plugin.render(DETAILS, None, &host.invocation()).unwrap();

    plugin.handle(DETAILS, ViewEvent::Button("close".into()), &host.invocation()).unwrap();

    assert!(writes(&host).is_empty(), "{:?}", writes(&host));
}

#[test]
fn showing_the_branch_merge_request_resets_the_pipeline_view() {
    let host = branch_on_merge_request_8();
    let mut plugin = selected(&host);
    plugin.pipeline_selection = Some(PipelineKey::Pipeline(41));
    plugin.pipeline_confirm = Some(41);
    plugin.pipeline_status = Some("Pipeline retried.".into());
    plugin.pipeline_open.insert("p:41".into(), false);
    plugin.handle_events(&[Event::VcsChanged("/project".into())], &host.invocation());
    let before = host.refreshed_views().len();

    plugin.run_command("show-branch-merge-request", &host.invocation(), None).unwrap();

    assert_eq!((plugin.pipeline_selection, plugin.pipeline_confirm, plugin.pipeline_status.as_deref()), (None, None, None));
    assert!(plugin.pipeline_open.is_empty());
    assert!(host.refreshed_views()[before..].contains(&LIST.to_owned()));
}

#[test]
fn another_action_or_selection_cancels_a_pending_close() {
    let host = setup();
    let mut plugin = selected(&host);

    plugin.handle(DETAILS, ViewEvent::Button("close".into()), &host.invocation()).unwrap();
    let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();
    assert_eq!(label(&view, "close"), "Confirm Close !7");

    plugin.handle(LIST, ViewEvent::Select("mr:7".into()), &host.invocation()).unwrap();
    let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();
    assert_eq!(label(&view, "close"), "Close");

    plugin.handle(DETAILS, ViewEvent::Button("close".into()), &host.invocation()).unwrap();
    plugin.handle(DETAILS, ViewEvent::Button("copy".into()), &host.invocation()).unwrap();
    plugin.handle(DETAILS, ViewEvent::Button("close".into()), &host.invocation()).unwrap();

    assert!(writes(&host).is_empty());
    let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();
    assert_eq!(label(&view, "close"), "Confirm Close !7");
}

#[test]
fn draft_uses_the_graphql_mutation() {
    let host = setup();
    let mut plugin = selected(&host);

    plugin.handle(DETAILS, ViewEvent::Button("draft".into()), &host.invocation()).unwrap();

    let posted = writes(&host);
    assert_eq!(posted[0].1, "https://gitlab.com/api/graphql");
    assert!(posted[0].2.contains(r#""variables":{"draft":true,"iid":"7","projectPath":"g/p"}"#));
    assert_eq!(plugin.status.as_deref(), Some("Marked as draft."));
}

#[test]
fn the_changes_view_shows_threads_on_their_lines() {
    let host = setup();
    let mut plugin = selected(&host);

    let view = changes_view(&mut plugin, &host).unwrap();

    let Body::Html(html) = &view.body else { panic!("{view:?}") };
    let row = html.find("id=\"f0--4\"").expect("the added line 4");
    let thread = html.find("Why?").unwrap();
    assert!(row < thread && thread < html.find("id=\"f0-5-5\"").unwrap());
    assert!(!html.contains("changed the description"));
}

#[test]
fn the_changes_page_reads_without_scripts() {
    let host = setup();
    let mut plugin = selected(&host);
    changes_view(&mut plugin, &host).unwrap();
    changes_message(&mut plugin, &host, ViewEvent::Message(r#"{"type":"viewed","path":"src/a.rs","viewed":true}"#.into())).unwrap();

    let view = changes_view(&mut plugin, &host).unwrap();

    let Body::Html(html) = &view.body else { panic!("{view:?}") };
    assert!(html.contains("<p class=\"nojs\" role=\"note\">Commenting on lines doesn't work in this panel"));
    assert!(html.contains(".js .collapsed table, .js .nojs { display: none; }"), "viewed files fold only when scripts run");
    assert!(html.contains("<script>\ndocument.documentElement.classList.add(\"js\");"));
    assert!(html.contains("src/a.rs") && html.contains("<td class=\"code\">c</td>"));
}

#[test]
fn comments_on_diff_lines_carry_the_position() {
    let host = setup();
    let mut plugin = selected(&host);
    changes_view(&mut plugin, &host).unwrap();

    let message =
        r#"{"type":"comment","oldPath":"src/a.rs","newPath":"src/a.rs","old":4,"new":null,"body":"gone?","draft":false,"anchor":"f0-4-"}"#;
    changes_message(&mut plugin, &host, ViewEvent::Message(message.into())).unwrap();
    let draft = r#"{"type":"comment","oldPath":"src/a.rs","newPath":"src/a.rs","old":3,"new":3,"body":"ctx","draft":true}"#;
    changes_message(&mut plugin, &host, ViewEvent::Message(draft.into())).unwrap();

    let posted = writes(&host);
    assert!(posted[0].1.ends_with("/merge_requests/7/discussions"));
    assert_eq!(
        posted[0].2,
        r#"{"body":"gone?","position":{"base_sha":"abcdef0","head_sha":"abcdef1","new_path":"src/a.rs","old_line":4,"old_path":"src/a.rs","position_type":"text","start_sha":"start"}}"#
    );
    assert!(posted[1].1.ends_with("/merge_requests/7/draft_notes"));
    assert!(posted[1].2.contains(r#""new_line":3"#) && posted[1].2.contains(r#""old_line":3"#));
    assert_eq!(plugin.status.as_deref(), Some("Added to your review."));
}

#[test]
fn a_failed_diff_comment_reports_to_the_page_without_redrawing_it() {
    let host = setup();
    host.on_fetch(|r| match r.method == "POST" {
        true => Ok(HttpResponse { status: 400, headers: vec![], body: br#"{"message":"line_code is invalid"}"#.to_vec() }),
        false => gitlab(r),
    });
    let mut plugin = selected(&host);
    changes_view(&mut plugin, &host).unwrap();
    let refreshed = host.refreshed_views().len();

    let message =
        r#"{"type":"comment","oldPath":"src/a.rs","newPath":"src/a.rs","old":4,"new":null,"body":"gone?","draft":false,"anchor":"f0-4-"}"#;
    changes_message(&mut plugin, &host, ViewEvent::Message(message.into())).unwrap();

    assert_eq!(host.refreshed_views().len(), refreshed);
    assert_eq!(plugin.status, None);
    assert_eq!(plugin.anchor, None);
    let messages = host.view_messages();
    let (view, posted) = messages.iter().find(|(_, text)| forwarded_message(text).contains(r#""type":"error""#)).unwrap();
    let posted=forwarded_message(posted);
    assert_eq!(view, LIST);
    assert!(posted.contains(r#""type":"error""#) && posted.contains("line_code is invalid"), "{posted}");
    assert!(host.view_messages().iter().all(|(view, _)| view == LIST));
}

#[test]
fn a_failing_section_keeps_the_section_buttons() {
    let host = setup();
    let mut plugin = selected(&host);
    host.on_fetch(|r| match r.url.contains("/diffs") {
        true => Ok(HttpResponse { status: 500, headers: vec![], body: br#"{"message":"boom"}"#.to_vec() }),
        false => gitlab(r),
    });

    let view = changes_view(&mut plugin, &host).unwrap();

    assert!(section_buttons(&view).iter().any(|(id, _)| *id == "section:overview"), "{view:?}");
    assert!(view.toolbar.iter().any(|b| b.id == "refresh"));
}

#[test]
fn viewed_files_are_remembered_for_the_head_commit() {
    let host = setup();
    let mut plugin = selected(&host);
    changes_view(&mut plugin, &host).unwrap();

    changes_message(&mut plugin, &host, ViewEvent::Message(r#"{"type":"viewed","path":"src/a.rs","viewed":true}"#.into())).unwrap();
    let view = changes_view(&mut plugin, &host).unwrap();

    assert_eq!(host.storage().get("viewed:g/p:7").map(String::as_str), Some(r#"{"src/a.rs":"abcdef1"}"#));
    let Body::Html(html) = &view.body else { panic!("{view:?}") };
    assert!(html.contains("class=\"file collapsed\""));
}

#[test]
fn the_current_branch_shows_its_merge_request_and_threads_in_the_editor() {
    let host = setup();
    host.set_vcs_status(Some(status("feature")));
    let mut plugin = GitLab::default();

    plugin.handle_events(&[Event::VcsChanged("/project".into()), Event::FileOpened("/project/src/a.rs".into())], &host.invocation());

    let item = &host.status_items()["merge-request"];
    assert_eq!(item.text, "!7 on feature");
    assert_eq!(item.command.as_deref(), Some("show-branch-merge-request"));
    assert!(host.requests().iter().any(|r| r.url.contains("source_branch=feature")));
    let diagnostics = &host.diagnostics()["/project/src/a.rs"];
    assert_eq!(diagnostics[0].message, "bob: Why? (1 reply)");
    assert_eq!(diagnostics[0].range.start.line, 3);
    let doc = Document { path: "/project/src/a.rs".into(), ..Document::default() };
    assert_eq!(plugin.line_markers(&doc)[0].line, 3);

    plugin.run_command("show-branch-merge-request", &host.invocation(), None).unwrap();
    assert_eq!(host.storage().get("selected").map(String::as_str), Some("7"));
}

#[test]
fn the_merge_request_item_menu_opens_only_the_branch_merge_request_links() {
    let host = setup();
    host.set_vcs_status(Some(status("feature")));
    let mut plugin = GitLab::default();
    plugin.handle_events(&[Event::VcsChanged("/project".into())], &host.invocation());

    let menu = host.status_menu("merge-request");
    let url = "https://gitlab.com/g/p/-/merge_requests/7";
    assert!(matches!(&menu[0], StatusMenuEntry::Header(label) if label.starts_with("!7 ")));
    assert_eq!(menu[1], StatusMenuEntry::action("Show Merge Request", "show-branch-merge-request"));
    assert_eq!(menu[2], StatusMenuEntry::action("Open Merge Request on GitLab", "open-in-browser").with_argument(url));
    let manifest = include_str!("../plugin.toml");
    for entry in &menu {
        if let StatusMenuEntry::Action { command, .. } = entry {
            assert!(manifest.contains(&format!("id = \"{command}\"")), "{command}");
        }
    }

    plugin.run_command("open-in-browser", &host.invocation(), Some(url)).unwrap();
    assert_eq!(host.opened_urls(), [url]);
    let _ = plugin.run_command("open-in-browser", &host.invocation(), Some("https://evil.example/"));
    assert!(!host.opened_urls().iter().any(|opened| opened.contains("evil")));
}

#[test]
fn a_pushed_branch_without_merge_request_offers_to_create_one() {
    let host = setup();
    host.set_vcs_status(Some(status("feature")));
    host.on_fetch(|r| match r.url.contains("source_branch=") {
        true => ok("[]"),
        false => gitlab(r),
    });

    GitLab::default().handle_events(&[Event::VcsChanged("/project".into())], &host.invocation());

    assert_eq!(host.status_items()["merge-request"].command.as_deref(), Some("create-merge-request"));

    host.set_vcs_status(Some(status("main")));
    GitLab::default().handle_events(&[Event::VcsChanged("/project".into())], &host.invocation());
    assert!(host.status_items().is_empty(), "nothing to propose on the default branch");
}

#[test]
fn check_out_fetches_and_tracks_the_source_branch() {
    let host = setup();
    host.on_process("git", |args, _| match args[0] == "rev-parse" {
        true => Ok(ProcessOutput { status: Some(1), stdout: String::new(), stderr: String::new() }),
        false => git_ok(args, None),
    });
    let mut plugin = selected(&host);

    plugin.handle(DETAILS, ViewEvent::Button("checkout".into()), &host.invocation()).unwrap();

    assert_eq!(
        git_runs(&host),
        [
            "fetch -- origin +refs/heads/feature:refs/remotes/origin/feature",
            "rev-parse --verify --quiet refs/heads/feature",
            "checkout -b feature --track origin/feature",
        ]
    );
    assert_eq!(plugin.status.as_deref(), Some("Checked out feature."));
}

fn fork_host(branch_exists: bool) -> MockHost {
    let host = setup();
    host.on_fetch(|r| match r.url.split('?').next() == Some("https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7") {
        true => ok(&MR.replace("\"source_project_id\": 5", "\"source_project_id\": 9")),
        false => gitlab(r),
    });
    host.on_process("git", move |args, _| match args[0] == "rev-parse" {
        true => Ok(ProcessOutput { status: Some(if branch_exists { 0 } else { 1 }), stdout: String::new(), stderr: String::new() }),
        false => git_ok(args, None),
    });
    host
}

#[test]
fn fork_checkout_confirms_existing_branch() {
    let host = fork_host(true);
    let mut plugin = selected(&host);
    plugin.handle(DETAILS, ViewEvent::Button("checkout".into()), &host.invocation()).unwrap();
    let runs = git_runs(&host);
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert!(runs[0].starts_with("rev-parse --verify --quiet refs/heads/fork/"), "{runs:?}");

    let host = fork_host(true);
    let mut plugin = selected(&host);
    host.answer_next_prompt(Some(&["ok"]));
    plugin.handle(DETAILS, ViewEvent::Button("checkout".into()), &host.invocation()).unwrap();
    assert!(git_runs(&host).iter().any(|r| r.starts_with("fetch -- origin +refs/merge-requests/7/head:refs/heads/fork/")), "{:?} {:?}", git_runs(&host), plugin.status);

    let host = fork_host(false);
    let mut plugin = selected(&host);
    plugin.handle(DETAILS, ViewEvent::Button("checkout".into()), &host.invocation()).unwrap();
    assert!(git_runs(&host).iter().any(|r| r.starts_with("fetch -- origin +refs/merge-requests/7/head:")));
}

#[test]
fn fork_checkout_treats_an_unexpected_rev_parse_status_as_an_error() {
    let host = fork_host(true);
    host.on_process("git", |_, _| Ok(ProcessOutput { status: None, stdout: String::new(), stderr: "killed".into() }));
    let mut plugin = selected(&host);

    let result = plugin.handle(DETAILS, ViewEvent::Button("checkout".into()), &host.invocation());

    assert!(git_runs(&host).iter().all(|r| !r.starts_with("fetch") && !r.starts_with("checkout")), "{:?}", git_runs(&host));
    assert!(result.is_err() || plugin.status.as_deref().is_some_and(|s| s.contains("rev-parse failed")), "{result:?} {:?}", plugin.status);
}

#[test]
fn check_out_without_git_says_so() {
    let without_git: Vec<&str> = CAPS.iter().copied().filter(|c| *c != "process:git").collect();
    let host = setup().with_capabilities(&without_git);
    let mut plugin = selected(&host);

    plugin.handle(DETAILS, ViewEvent::Button("checkout".into()), &host.invocation()).unwrap();

    assert_eq!(plugin.status.as_deref(), Some("Checking out a branch needs the process:git permission. Allow it in Settings › Plugins › GitLab."));
}

#[test]
fn clones_next_to_the_current_project_in_a_terminal() {
    let host = setup();
    let mut plugin = GitLab::default();
    plugin.render(PROJECTS, None, &host.invocation()).unwrap();

    plugin.handle(PROJECTS, ViewEvent::Activate("me/tool".into()), &host.invocation()).unwrap();

    let terminal = &host.terminals()[0];
    assert_eq!(terminal.args, ["clone", "--", "https://gitlab.com/me/tool.git", "/tool"]);

    host.set_setting("cloneDirectory", r#""/src""#);
    host.set_setting("cloneUsingSsh", "true");
    plugin.handle(PROJECTS, ViewEvent::Activate("me/tool".into()), &host.invocation()).unwrap();
    assert_eq!(host.terminals()[1].args, ["clone", "--", "git@gitlab.com:me/tool.git", "/src/tool"]);
}

#[test]
fn clone_puts_the_url_and_target_after_double_dash() {
    let host = setup();
    host.set_setting("cloneDirectory", r#""/work""#);
    let mut plugin = GitLab::default();
    plugin.render(PROJECTS, None, &host.invocation()).unwrap();

    plugin.handle(PROJECTS, ViewEvent::Activate("me/tool".into()), &host.invocation()).unwrap();

    assert_eq!(host.terminals()[0].args, ["clone", "--", "https://gitlab.com/me/tool.git", "/work/tool"]);
}

fn projects_with(path: &'static str, url: &'static str) -> impl Fn(&HttpRequest) -> Result<HttpResponse, Error> {
    move |r| match r.url.contains("membership=true") {
        true => ok(&format!(
            r#"[{{"name": "Tool", "path": "{path}", "path_with_namespace": "me/tool", "name_with_namespace": "Me / Tool",
                "http_url_to_repo": "{url}", "ssh_url_to_repo": "git@gitlab.com:me/tool.git"}}]"#
        )),
        false => gitlab(r),
    }
}

#[test]
fn clone_refuses_a_project_path_starting_with_a_dash() {
    let host = setup();
    for (path, url) in [
        ("-x", "https://gitlab.com/me/tool.git"),
        ("--upload-pack=x", "https://gitlab.com/me/tool.git"),
        ("tool", "https://gitlab.com/me/tool.git;rm -rf ~"),
    ] {
        host.on_fetch(projects_with(path, url));
        let mut plugin = GitLab::default();
        plugin.render(PROJECTS, None, &host.invocation()).unwrap();

        let result = plugin.handle(PROJECTS, ViewEvent::Activate("me/tool".into()), &host.invocation());

        assert!(result.is_err(), "{path} {url}");
    }
    assert!(host.terminals().is_empty());
}

#[test]
fn share_refuses_a_branch_name_that_reads_as_an_option() {
    let host = setup();
    host.set_vcs_status(Some(status("--upload-pack=x")));
    host.set_vcs_remotes(&[("origin", "git@github.com:x/y.git")]);
    host.on_process("git", git_ok);
    host.answer_next_prompt(Some(&[""]));
    host.answer_next_prompt(Some(&["private"]));

    let result = GitLab::default().run_command("share-project", &host.invocation(), None);

    assert!(result.is_err());
    assert!(writes(&host).is_empty() && host.terminals().is_empty() && host.process_runs().is_empty());
}

#[test]
fn reply_and_resolve_encode_the_discussion_id() {
    let host = setup();
    let api = api::Project("g/p");

    for id in ["abc/notes", "abc?x=1", "abc#x", "..", "%2F"] {
        assert!(api.reply(7, id, "x").is_err(), "{id}");
        assert!(api.resolve(7, id, true).is_err(), "{id}");
    }
    assert!(host.requests().is_empty());

    api.reply(7, "abc123", "x").unwrap();
    api.resolve(7, "abc123", false).unwrap();

    let posted = writes(&host);
    assert_eq!(posted[0].1, "https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7/discussions/abc123/notes");
    assert_eq!(posted[1].1, "https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7/discussions/abc123?resolved=false");
}

#[test]
fn a_draft_reply_to_a_hostile_discussion_id_sends_nothing() {
    let host = setup();
    let api = api::Project("g/p");

    for id in ["abc/notes", "abc?x=1", "abc#x", "..", "%2F", ""] {
        assert!(api.create_draft(7, "x", None, Some(id)).is_err(), "{id}");
    }
    assert!(host.requests().is_empty());

    api.create_draft(7, "x", None, Some("abc123")).unwrap();

    assert!(writes(&host)[0].2.contains(r#""in_reply_to_discussion_id":"abc123""#));
}

#[test]
fn the_token_never_appears_in_requests_logs_notifications_or_views() {
    let host = setup();
    host.on_process("git", git_ok);
    let mut plugin = GitLab::default();
    let mut views = Vec::new();

    views.push(format!("{:?}", plugin.render(LIST, None, &host.invocation()).unwrap()));
    plugin.handle(LIST, ViewEvent::Select("mr:7".into()), &host.invocation()).unwrap();
    views.push(format!("{:?}", plugin.render(DETAILS, None, &host.invocation()).unwrap()));
    views.push(format!("{:?}", changes_view(&mut plugin, &host).unwrap()));
    plugin.handle(DETAILS, field("comment", "LGTM"), &host.invocation()).unwrap();
    plugin.handle(DETAILS, ViewEvent::Button("comment".into()), &host.invocation()).unwrap();
    views.push(format!("{:?}", plugin.render(DETAILS, None, &host.invocation()).unwrap()));
    views.push(format!("{:?}", plugin.render(PROJECTS, None, &host.invocation()).unwrap()));
    plugin.handle(PROJECTS, ViewEvent::Activate("me/tool".into()), &host.invocation()).unwrap();
    host.open_editor("/project/src/a.rs", "rust", "fn main() {}\n", &[(0, 0, 0, 0)]);
    for answer in [Some(&[""][..]), Some(&["private"]), Some(&[""]), Some(&[""]), Some(&["private"])] {
        host.answer_next_prompt(answer);
    }
    plugin.run_command("create-snippet", &host.invocation(), None).unwrap();
    host.set_vcs_remotes(&[("origin", "git@github.com:x/y.git")]);
    GitLab::default().run_command("share-project", &host.invocation(), None).unwrap();

    assert_eq!(api::authorization("https://gitlab.com").unwrap(), "Bearer {{secret:token}}");
    let requests = host.requests();
    assert!(requests.len() >= 5, "{requests:?}");
    for request in &requests {
        let authorization: Vec<_> = request.headers.iter().filter(|(name, _)| name == "Authorization").collect();
        assert_eq!(authorization, [&("Authorization".to_owned(), "Bearer glpat-x".to_owned())], "{}", request.url);
        assert!(!request.url.contains("glpat-x"));
        assert!(!String::from_utf8_lossy(request.body.as_deref().unwrap_or_default()).contains("glpat-x"));
        assert!(request.headers.iter().filter(|(name, _)| name != "Authorization").all(|(_, value)| !value.contains("glpat-x")));
    }
    assert!(host.logs().iter().all(|(_, line)| !line.contains("glpat-x")));
    assert!(host.notifications().iter().all(|n| !format!("{n:?}").contains("glpat-x")));
    assert!(host.terminals().iter().all(|t| !format!("{t:?}").contains("glpat-x")));
    assert!(host.process_runs().iter().all(|r| !format!("{r:?}").contains("glpat-x")));
    assert!(views.iter().all(|v| !v.contains("glpat-x")));
    assert!(!host.terminals().is_empty() && !host.notifications().is_empty());
}

#[test]
fn an_error_response_is_shown_without_markup_or_over_200_characters() {
    let host = setup();
    let page = "<!DOCTYPE html><html><head><title>GitLab is not responding (502)</title><style>body { color: red; }</style></head><body><h1>502</h1></body></html>";
    let long = format!(r#"{{"message":"<b>{}</b>"}}"#, "x".repeat(500));
    for body in [page.to_owned(), long, format!("<p>{}</p>", "y".repeat(500))] {
        host.on_fetch(move |_| Ok(HttpResponse { status: 502, headers: vec![], body: body.clone().into_bytes() }));

        let message = api::current_user().unwrap_err().to_string();

        assert!(!message.contains('<') && !message.contains('>') && !message.contains('{'), "{message}");
        assert!(message.chars().count() <= 200, "{}", message.chars().count());
        assert!(message.starts_with("GitLab returned 502: "), "{message}");
    }

    host.on_fetch(move |_| Ok(HttpResponse { status: 502, headers: vec![], body: page.as_bytes().to_vec() }));
    assert_eq!(api::current_user().unwrap_err().to_string(), "GitLab returned 502: GitLab is not responding (502)");
}

#[test]
fn no_projects_shows_a_message() {
    let host = setup();
    host.on_fetch(|r| match r.url.contains("membership=true") {
        true => ok("[]"),
        false => gitlab(r),
    });

    let view = GitLab::default().render(PROJECTS, None, &host.invocation()).unwrap();

    assert_eq!(view.body, Body::Message("You aren't a member of any project.".into()));
    assert_eq!(buttons(&view), ["refresh"]);
}

#[test]
fn share_creates_the_project_adds_the_remote_and_pushes() {
    let host = setup();
    host.set_vcs_remotes(&[("origin", "git@github.com:x/y.git")]);
    host.on_process("git", git_ok);
    host.answer_next_prompt(Some(&[""]));
    host.answer_next_prompt(Some(&["private"]));

    GitLab::default().run_command("share-project", &host.invocation(), None).unwrap();

    assert_eq!(writes(&host)[0].2, r#"{"name":"project","visibility":"private"}"#);
    assert_eq!(git_runs(&host), ["--version", "remote add -- gitlab https://gitlab.com/me/project.git"]);
    assert_eq!(host.terminals()[0].args, ["push", "-u", "--", "gitlab", "main"]);
}

#[test]
fn opens_and_copies_the_file_link_at_head() {
    let host = setup();
    host.open_editor("/project/src/a b.rs", "rust", "one\ntwo\nthree\n", &[(1, 0, 3, 0)]);

    GitLab::default().run_command("open-in-browser", &host.invocation(), None).unwrap();
    GitLab::default().run_command("copy-link", &host.invocation(), None).unwrap();

    let url = "https://gitlab.com/g/p/-/blob/abc123/src/a%20b.rs#L2-L3";
    assert_eq!(host.opened_urls(), [url]);
    assert_eq!(host.clipboard().as_deref(), Some(url));
}

#[test]
fn copy_link_without_clipboard_notifies() {
    let host = setup().with_capabilities(&["vcs.read", "editor.read"]);
    host.open_editor("/project/a.rs", "rust", "x", &[(0, 0, 0, 0)]);

    GitLab::default().run_command("copy-link", &host.invocation(), None).unwrap();

    assert_eq!(
        host.notifications()[0].body,
        "Copying the link needs the clipboard permission. Allow it in Settings › Plugins › GitLab.\nhttps://gitlab.com/g/p/-/blob/abc123/a.rs#L1"
    );
}

#[test]
fn creates_a_private_snippet_from_the_selection() {
    let host = setup();
    host.open_editor("/project/src/a.rs", "rust", "fn main() {}\n", &[(0, 3, 0, 7)]);
    for answer in [Some(&[""][..]), Some(&["private"]), Some(&[""])] {
        host.answer_next_prompt(answer);
    }

    GitLab::default().run_command("create-snippet", &host.invocation(), None).unwrap();

    let posted = writes(&host);
    assert_eq!(posted[0].2, r#"{"files":[{"content":"main","file_path":"src/a.rs"}],"title":"a.rs","visibility":"private"}"#);
    assert_eq!(host.clipboard().as_deref(), Some("https://gitlab.com/-/snippets/9"));
    assert_eq!(host.notifications()[0].title, "Snippet created");
}

#[test]
fn tree_keys_round_trip() {
    assert_eq!(Key::parse("mr:7"), Some(Key::MergeRequest(7)));
    assert_eq!(Key::parse("d:7:abc"), Some(Key::Thread(7, "abc")));
    assert_eq!(Key::parse("n:7:abc:12"), Some(Key::Note(7, "abc", 12)));
    assert_eq!(Key::parse("e:7:0"), None);
}

#[test]
fn second_page_is_fetched_when_first_is_full() {
    let host = setup();
    host.on_fetch(|req| {
        let item = r#"{"iid": 1}"#;
        let count = if req.url.contains("page=1") { 50 } else { 1 };

        ok(&format!("[{}]", vec![item; count].join(",")))
    });

    let merge_requests = api::Project("g/p").merge_requests("state=opened").unwrap();

    assert_eq!(merge_requests.len(), 51);
    assert_eq!(host.requests().len(), 2);
}

#[test]
fn paging_stops_at_the_page_cap() {
    let host = setup();
    host.on_fetch(|_| ok(&format!("[{}]", vec![r#"{"id": "d", "notes": [{"id": 1}]}"#; 100].join(","))));

    let threads = api::Project("g/p").discussions(7).unwrap();

    assert_eq!(threads.len(), 1000);
    assert_eq!(host.requests().len(), 10);
}

fn file_changed_at_c1(args: &[String], _: Option<&str>) -> Result<ProcessOutput, Error> {
    let stdout = match args.iter().any(|a| a == "log") {
        true => "\u{1}c2\n\nsrc/b.rs\n\u{1}c1\n\nsrc/a.rs\nsrc/b.rs\n".to_owned(),
        false => String::new(),
    };

    Ok(ProcessOutput { status: Some(0), stdout, stderr: String::new() })
}

fn changes_html(plugin: &mut GitLab, host: &MockHost) -> String {
    let view = changes_view(plugin, &host).unwrap();
    let Body::Html(html) = view.body else { panic!("{view:?}") };

    html
}

#[test]
fn viewed_compares_file_last_commit() {
    let host = setup();
    host.on_process("git", file_changed_at_c1);
    let mut plugin = selected(&host);

    storage::set("viewed:g/p:7", r#"{"src/a.rs": "c1"}"#).unwrap();
    assert!(changes_html(&mut plugin, &host).contains(" checked"), "a.rs last changed at c1, viewed at c1");

    storage::set("viewed:g/p:7", r#"{"src/a.rs": "c0"}"#).unwrap();
    assert!(!changes_html(&mut plugin, &host).contains(" checked"), "viewed before the last change to a.rs");

    changes_message(&mut plugin, &host, ViewEvent::Message(r#"{"type":"viewed","path":"src/a.rs","viewed":true}"#.into())).unwrap();
    assert_eq!(storage::get("viewed:g/p:7").unwrap().unwrap(), r#"{"src/a.rs":"c1"}"#);
}

#[test]
fn local_drafts_tab_flags_stale_heads_and_submits_as_review_drafts() {
    let host = setup();
    let mut plugin = selected(&host);
    let mut store = local_drafts::Store::load("g/p", 7).unwrap();
    store.add("abcdef1", "src/a.rs", 4, "unsent").unwrap();
    store.save().unwrap();
    let view = render_in(&mut plugin, Section::Drafts, &host);
    let Body::Other { data, .. } = &view.body else { panic!("{view:?}") };
    assert!(data.contains("Ready") && data.contains("unsent"), "{data}");
    let mut stale = local_drafts::Store::load("g/p", 7).unwrap();
    stale.rebind("old");
    stale.save().unwrap();
    let view = render_in(&mut plugin, Section::Drafts, &host);
    let Body::Other { data, .. } = &view.body else { panic!("{view:?}") };
    assert!(data.contains("STALE"), "{data}");
    assert!(view.status.unwrap().contains("STALE"));
    storage::set(sections::SECTION_KEY, "drafts").unwrap();
    assert!(plugin.handle(DETAILS, ViewEvent::Button("draft-submit".into()), &host.invocation()).is_err());
    assert_eq!(local_drafts::Store::load("g/p", 7).unwrap().items.len(), 1);
}

#[test]
fn files_view_toggles_viewed_without_page_scripts_and_switches_layout() {
    let host = setup();
    host.on_process("git", |_, _| Ok(ProcessOutput { status: Some(128), stdout: String::new(), stderr: "bad object".into() }));
    let mut plugin = selected(&host);
    let items = |plugin: &mut GitLab| {
        let view = render_in(plugin, Section::Files, &host);
        let Body::Tree(items) = view.body else { panic!("{view:?}") };

        items
    };

    let tree = items(&mut plugin);
    assert_eq!(tree.len(),2);assert_eq!(tree[0].key,"dir:src");
    let (_,path): (String,String)=serde_json::from_str(tree[1].key.strip_prefix("file:").unwrap()).unwrap();assert_eq!(path,"src/a.rs");
    assert_eq!(tree[1].parent.as_deref(), Some("dir:src"));
    assert!(tree[1].icon.is_none());

    handle_in(&mut plugin, Section::Files, ViewEvent::Activate(tree[1].key.clone()), &host);

    assert_eq!(storage::get("viewed:g/p:7").unwrap().unwrap(), r#"{"src/a.rs":"abcdef1"}"#);
    assert_eq!(items(&mut plugin)[1].icon.as_deref(), Some("check"));
    assert!(changes_html(&mut plugin, &host).contains(" checked"));

    handle_in(&mut plugin, Section::Files, ViewEvent::Button("layout".into()), &host);
    let list = items(&mut plugin);
    assert_eq!((list.len(), list[0].label.as_str(), list[0].parent.as_deref()), (1, "src/a.rs", None));

    handle_in(&mut plugin, Section::Files, ViewEvent::Activate("file:nope".into()), &host);
    handle_in(&mut plugin, Section::Files, ViewEvent::Button("clear".into()), &host);
    assert_eq!(storage::get("viewed:g/p:7").unwrap().unwrap(), "{}");

    handle_in(&mut plugin, Section::Files, ViewEvent::Button("mark-all".into()), &host);
    assert_eq!(storage::get("viewed:g/p:7").unwrap().unwrap(), r#"{"src/a.rs":"abcdef1"}"#);
}

#[test]
fn viewed_falls_back_to_the_head_without_local_history() {
    let host = setup();
    host.on_process("git", |_, _| Ok(ProcessOutput { status: Some(128), stdout: String::new(), stderr: "bad object".into() }));
    let mut plugin = selected(&host);

    changes_view(&mut plugin, &host).unwrap();
    changes_message(&mut plugin, &host, ViewEvent::Message(r#"{"type":"viewed","path":"src/a.rs","viewed":true}"#.into())).unwrap();

    assert_eq!(storage::get("viewed:g/p:7").unwrap().unwrap(), r#"{"src/a.rs":"abcdef1"}"#);
    assert!(changes_html(&mut plugin, &host).contains(" checked"));
}

#[test]
fn non_hex_shas_are_not_passed_to_git() {
    let host = setup();
    let git_calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let counter = git_calls.clone();
    host.on_process("git", move |_, _| {
        counter.set(counter.get() + 1);
        Ok(ProcessOutput { status: Some(0), stdout: String::new(), stderr: String::new() })
    });
    host.on_fetch(|req| match req.url.ends_with("/merge_requests/7") {
        true => ok(&MR.replace("\"abcdef0\"", "\"--output=/tmp/x\"")),
        false => gitlab(req),
    });
    let mut plugin = selected(&host);

    changes_view(&mut plugin, &host).unwrap();
    changes_message(&mut plugin, &host, ViewEvent::Message(r#"{"type":"viewed","path":"src/a.rs","viewed":true}"#.into())).unwrap();

    assert_eq!(git_calls.get(), 0);
    assert_eq!(storage::get("viewed:g/p:7").unwrap().unwrap(), r#"{"src/a.rs":"abcdef1"}"#);
}

fn branch_with_suggestion(body: &str) -> (MockHost, GitLab) {
    let host = setup();
    host.set_vcs_status(Some(status("feature")));
    let discussions = serde_json::json!([{"id": "d1", "notes": [
        {"id": 11, "body": body, "author": {"username": "bob"}, "resolvable": true, "resolved": false,
         "position": {"new_path": "src/a.rs", "new_line": 4}}
    ]}])
    .to_string();
    host.on_fetch(move |req| match req.url.contains("/discussions") {
        true => ok(&discussions),
        false => gitlab(req),
    });
    let mut plugin = GitLab::default();
    plugin.handle_events(&[Event::VcsChanged("/project".into())], &host.invocation());

    (host, plugin)
}

fn document(path: &str) -> Document {
    Document { path: path.into(), text: "a\nb\nc\nd\ne\nf\n".into(), ..Document::default() }
}

fn at_line(line: u32) -> openidea_plugin::Range {
    openidea_plugin::Range::new(openidea_plugin::Position::new(line, 0), openidea_plugin::Position::new(line, 0))
}

#[test]
fn suggestion_block_becomes_edit() {
    let (_host, mut plugin) = branch_with_suggestion("Better:\n```suggestion:-1+1\nX\nY\n```\n");

    let actions = plugin.code_actions(&document("/project/src/a.rs"), at_line(3));

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].title, "GitLab: Apply suggestion");
    let (path, edits) = &actions[0].edits[0];
    assert_eq!(path, "/project/src/a.rs");
    assert_eq!((edits[0].range.start.line, edits[0].range.end.line), (2, 5));
    assert_eq!(edits[0].text, "X\nY\n");

    assert!(plugin.code_actions(&document("/project/src/a.rs"), at_line(0)).is_empty(), "not on the commented line");
    assert!(plugin.code_actions(&document("/project/src/b.rs"), at_line(3)).is_empty(), "not the commented file");
}

#[test]
fn a_plain_suggestion_replaces_the_commented_line_and_an_empty_one_deletes_it() {
    let (_host, mut plugin) = branch_with_suggestion("```suggestion\nD\n```\n```suggestion:-0+0\n```");

    let actions = plugin.code_actions(&document("/project/src/a.rs"), at_line(3));

    let edits: Vec<_> =
        actions.iter().map(|a| (a.edits[0].1[0].range.start.line, a.edits[0].1[0].range.end.line, a.edits[0].1[0].text.as_str())).collect();
    assert_eq!(edits, [(3, 4, "D\n"), (3, 4, "")]);
}

#[test]
fn a_suggestion_reaching_past_the_file_is_not_offered() {
    let (_host, mut plugin) = branch_with_suggestion("```suggestion:-0+9\nX\n```");

    assert!(plugin.code_actions(&document("/project/src/a.rs"), at_line(3)).is_empty());
}

#[test]
fn paging_stops_at_the_deadline() {
    let host = setup();
    host.on_fetch(|_| ok(&format!("[{}]", vec![r#"{"id": "d", "notes": [{"id": 1}]}"#; 100].join(","))));
    let clock = std::cell::Cell::new(0);
    let now = || {
        clock.set(clock.get() + 20);
        clock.get()
    };

    let threads: Vec<Discussion> = api::paged_until("/projects/g%2Fp/merge_requests/7/discussions", 100, now, 45).unwrap();

    assert_eq!(threads.len(), 200, "the deadline is 65 s; checks read 40 s, 60 s, 80 s");
    assert_eq!(host.requests().len(), 2);
}

#[test]
fn suggestion_edit_keeps_crlf() {
    let (_host, mut plugin) = branch_with_suggestion("```suggestion\nX\nY\n```");
    let doc = Document { text: "a\r\nb\r\nc\r\nd\r\ne\r\n".into(), ..document("/project/src/a.rs") };

    let actions = plugin.code_actions(&doc, at_line(3));

    assert_eq!(actions[0].edits[0].1[0].text, "X\r\nY\r\n");
}

#[test]
fn suggestion_on_a_last_line_without_newline_adds_none() {
    let (_host, mut plugin) = branch_with_suggestion("```suggestion\nX\n```");
    let doc = Document { text: "a\nb\nc\nd".into(), ..document("/project/src/a.rs") };

    let edit = &plugin.code_actions(&doc, at_line(3))[0].edits[0].1[0];

    assert_eq!(edit.text, "X");
    assert_eq!((edit.range.end.line, edit.range.end.character), (3, 1));
}

#[test]
fn suggestion_is_not_offered_when_the_line_differs_from_the_cached_diff() {
    let (_host, mut plugin) = branch_with_suggestion("```suggestion\nX\n```");
    plugin.diffs = Some((
        7,
        vec![FileDiff {
            old_path: "src/a.rs".into(),
            new_path: "src/a.rs".into(),
            diff: "@@ -3,2 +3,2 @@\n c\n+d\n".into(),
            ..FileDiff::default()
        }],
    ));

    assert_eq!(plugin.code_actions(&document("/project/src/a.rs"), at_line(3)).len(), 1, "line 4 is `d` in the diff and in the file");

    let edited = Document { text: "a\nb\nc\nchanged\ne\nf\n".into(), ..document("/project/src/a.rs") };
    assert!(plugin.code_actions(&edited, at_line(3)).is_empty());
}

#[test]
fn the_viewed_history_is_read_with_unquoted_paths() {
    let host = setup();
    host.on_process("git", file_changed_at_c1);
    let mut plugin = selected(&host);

    changes_html(&mut plugin, &host);

    assert!(git_runs(&host)[0].starts_with("-c core.quotepath=off log "), "{:?}", git_runs(&host));
}

#[test]
fn suggestion_guard_checks_the_whole_range() {
    let (_host, mut plugin) = branch_with_suggestion("```suggestion:-1+0\nX\n```");
    plugin.diffs = Some((
        7,
        vec![FileDiff {
            old_path: "src/a.rs".into(),
            new_path: "src/a.rs".into(),
            diff: "@@ -3,2 +3,2 @@\n c\n+d\n".into(),
            ..FileDiff::default()
        }],
    ));

    assert_eq!(plugin.code_actions(&document("/project/src/a.rs"), at_line(3)).len(), 1);

    let edited = Document { text: "a\nb\nchanged\nd\ne\nf\n".into(), ..document("/project/src/a.rs") };
    assert!(plugin.code_actions(&edited, at_line(3)).is_empty(), "the line above the comment changed");
}

#[test]
fn crlf_suggestion_on_a_last_line_without_newline_keeps_no_carriage_return() {
    let (_host, mut plugin) = branch_with_suggestion("```suggestion\r\nX\r\n```");
    let doc = Document { text: "a\r\nb\r\nc\r\nd".into(), ..document("/project/src/a.rs") };

    assert_eq!(plugin.code_actions(&doc, at_line(3))[0].edits[0].1[0].text, "X");
}

#[test]
fn second_list_in_the_same_task_respects_the_shared_deadline() {
    let host = setup();
    host.on_fetch(|_| ok(&format!("[{}]", vec![r#"{"id": "d", "notes": [{"id": 1}]}"#; 100].join(","))));
    let now = || if host.requests().is_empty() { 0 } else { 36 };
    api::begin_budget_at(0);

    let first: Vec<Discussion> = api::paged_until("/a", 100, now, 35).unwrap();
    let second: Vec<Discussion> = api::paged_until("/b", 100, now, 35).unwrap();

    assert_eq!((first.len(), second.len()), (100, 0));
    assert_eq!(host.requests().len(), 1);
}

mod ci_tests {
    use super::*;
    use crate::ci;
    use openidea_plugin::{Position, text};

    fn findings(yaml: &str) -> Vec<ci::Finding> {
        ci::check(&ci::parse(yaml))
    }

    fn codes(yaml: &str) -> Vec<&'static str> {
        findings(yaml).iter().map(|f| f.code).collect()
    }

    fn open(host: &MockHost, plugin: &mut GitLab, yaml: &str) -> Vec<openidea_plugin::Diagnostic> {
        host.add_file("/project/.gitlab-ci.yml", yaml);
        plugin.handle_events(&[Event::FileSaved("/project/.gitlab-ci.yml".into())], &host.invocation());
        host.diagnostics().get("/project/.gitlab-ci.yml").cloned().unwrap_or_default()
    }

    #[test]
    fn failed_read_clears_stale_ci_problems() {
        let host = MockHost::install();
        let mut plugin = GitLab::default();
        let path = "/project/gone.gitlab-ci.yml";
        let stale = openidea_plugin::Diagnostic::new(
            Range::new(Position::new(0, 0), Position::new(0, 1)),
            openidea_plugin::Severity::Warning,
            "old",
        );
        plugin.ci_problems.insert(path.into(), vec![stale]);
        plugin.publish_review(path);
        assert_eq!(host.diagnostics()[path].len(), 1);

        plugin.handle_events(&[Event::FileSaved(path.into())], &host.invocation());

        assert!(host.diagnostics().get(path).is_none_or(Vec::is_empty));
    }

    #[test]
    fn needs_unknown_job_warns() {
        let yaml = "build:\n  script: [make]\ntest:\n  needs:\n    - build\n    - biuld\n  script: [make test]\n";
        let found = findings(yaml);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code, "undefined-job");
        assert_eq!(&yaml[found[0].start..found[0].end], "biuld");
    }

    #[test]
    fn needs_inline_and_mapping_forms_are_read() {
        let yaml = "a:\n  script: [x]\nb:\n  needs: [a, nope, {job: nada}]\n  script: [x]\nc:\n  needs:\n    - job: gone\n      artifacts: false\n  script: [x]\n";

        assert_eq!(codes(yaml), vec!["undefined-job"; 3]);
    }

    #[test]
    fn undefined_job_is_quiet_for_library_or_dynamic_name() {
        let yaml = "a:\n  needs:\n    - project: g/p\n      job: other\n      ref: main\n    - pipeline: $PARENT\n      job: x\n    - $DYNAMIC\n    - job: \"${NAME}\"\n    - *alias\n  extends: !reference [.x]\n  script: [x]\n.x:\n  script: [y]\n";

        assert!(codes(yaml).is_empty(), "{:?}", findings(yaml));
    }

    #[test]
    fn extends_hidden_job_is_defined() {
        let yaml = ".tpl: &t\n  script: [x]\njob:\n  extends: .tpl\n";

        assert!(codes(yaml).is_empty());
        assert_eq!(codes("job:\n  extends: .missing\n"), vec!["undefined-job"]);
        assert_eq!(codes("job:\n  extends:\n    - .a\n  script: [x]\n"), vec!["undefined-job"]);
    }

    #[test]
    fn dependencies_unknown_job_warns() {
        assert_eq!(codes("a:\n  dependencies: [ghost]\n  script: [x]\n"), vec!["undefined-job"]);
    }

    #[test]
    fn stage_not_in_stages_warns() {
        let yaml = "stages:\n  - build\n  - test\njob:\n  stage: deploy\n  script: [x]\n";
        let found = findings(yaml);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code, "undefined-stage");
        assert_eq!(&yaml[found[0].start..found[0].end], "deploy");
    }

    #[test]
    fn default_stages_are_known_when_stages_absent() {
        let known = "a:\n  stage: .pre\nb:\n  stage: build\nc:\n  stage: test\nd:\n  stage: deploy\ne:\n  stage: .post\n";

        assert!(codes(known).is_empty());
        assert_eq!(codes("a:\n  stage: package\n"), vec!["undefined-stage"]);
        assert_eq!(codes("stages: [lint, ship]\na:\n  stage: ship\nb:\n  stage: build\n"), vec!["undefined-stage"]);
    }

    #[test]
    fn undefined_stage_is_quiet_for_library_or_dynamic_name() {
        assert!(codes("a:\n  stage: $STAGE\nb:\n  stage: \"${S}\"\n").is_empty());
        assert!(codes("include: other.yml\na:\n  stage: custom\n").is_empty());
    }

    #[test]
    fn duplicate_needs_entry_warns() {
        let yaml = "a:\n  script: [x]\nb:\n  needs: [a, a]\n  script: [x]\nc:\n  extends:\n    - .t\n    - .t\n";
        let dup: Vec<_> = findings(yaml).into_iter().filter(|f| f.code == "duplicate-job-usage").collect();

        assert_eq!(dup.len(), 2);
        assert_eq!(dup[0].start, yaml.rfind("a]").unwrap());
    }

    #[test]
    fn single_quoted_dollar_variable_in_script_warns() {
        let yaml =
            "job:\n  script:\n    - echo 'deploying $CI_COMMIT_SHA'\n    - printf '${HOME}\\n'\n  after_script:\n    - echo -n '$X'\n";
        let found = findings(yaml);

        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found.iter().all(|f| f.code == "single-quoted-shell-variable"));
        assert_eq!(&yaml[found[0].start..found[0].end], "$CI_COMMIT_SHA");
    }

    #[test]
    fn double_quoted_variable_is_quiet() {
        let yaml = "job:\n  script:\n    - echo \"deploying $CI_COMMIT_SHA\"\n    - echo 'plain'\n    - sh -c 'echo $HOME'\n    - echo '$A' \"$B\"\n    - 'echo $C'\n    - echo $D\n";

        assert!(codes(yaml).is_empty(), "{:?}", findings(yaml));
    }

    #[test]
    fn include_present_makes_unknown_jobs_quiet() {
        let yaml = "include:\n  - local: other.yml\njob:\n  extends: .from-include\n  needs: [elsewhere]\n";

        assert!(codes(yaml).is_empty());
    }

    #[test]
    fn comments_anchors_merges_and_reserved_keys_are_quiet() {
        let yaml = "# top\nvariables:\n  A: b\nimage: ruby\ndefault:\n  before_script: [x]\nworkflow:\n  rules: []\n.base: &base\n  script: [x]\njob:\n  <<: *base\n  needs: [pages] # trailing\npages:\n  script: [x]\n";

        assert!(codes(yaml).is_empty(), "{:?}", findings(yaml));
    }

    #[test]
    fn tabs_and_odd_shapes_yield_no_findings() {
        assert!(codes("job:\n\tneeds: [x]\n").is_empty());
        assert!(codes("not yaml at all [").is_empty());
        assert!(codes("").is_empty());
        assert!(codes("- a\n- b\n").is_empty());
    }

    #[test]
    fn ranges_use_utf16_columns_with_multibyte_text() {
        let yaml = "bü🦀ild:\n  script: [x]\ntest:\n  needs: [\"é🦀\", nope]\n";
        let found = findings(yaml);
        let range = text::position(yaml, found[1].start);

        assert_eq!(found.len(), 2);
        assert_eq!(range, Position::new(3, 17));
    }

    #[test]
    fn crlf_files_report_correct_lines() {
        let yaml = "a:\r\n  script: [x]\r\nb:\r\n  needs:\r\n    - ghost\r\n";
        let found = findings(yaml);

        assert_eq!(found.len(), 1);
        assert_eq!(&yaml[found[0].start..found[0].end], "ghost");
        assert_eq!(text::position(yaml, found[0].start), Position::new(4, 6));
    }

    #[test]
    fn multiline_flow_stages_is_quiet() {
        assert!(codes("stages: [build,\n  test]\njob:\n  stage: test\n").is_empty());
    }

    #[test]
    fn aliased_stages_is_quiet() {
        assert!(codes("x: &s [a]\nstages: *s\njob:\n  stage: a\n").is_empty());
    }

    #[test]
    fn tagged_stages_is_quiet() {
        assert!(codes(".s:\n  script: x\nstages: !reference [.s]\njob:\n  stage: a\n").is_empty());
    }

    #[test]
    fn flow_mapping_job_counts_as_defined() {
        assert!(codes("build: {script: [make]}\ntest:\n  needs: [build]\n  extends: build\n").is_empty());
    }

    #[test]
    fn tagged_job_counts_as_defined() {
        assert!(codes(".x:\n  y: 1\nbuild: !reference [.x, y]\ntest:\n  needs: [build]\n").is_empty());
    }

    #[test]
    fn review_thread_and_ci_problem_on_same_path_are_both_published() {
        let host = setup();
        host.set_vcs_status(Some(status("feature")));
        host.add_file("/project/.gitlab-ci.yml", "job:\n  needs: [ghost]\n");
        host.on_fetch(|r| match r.url.contains("/discussions") {
            true => ok(&DISCUSSIONS.replace("src/a.rs", ".gitlab-ci.yml")),
            false => gitlab(r),
        });
        let path = "/project/.gitlab-ci.yml";

        GitLab::default().handle_events(&[Event::VcsChanged("/project".into()), Event::FileOpened(path.into())], &host.invocation());

        let messages: Vec<_> = host.diagnostics()[path].iter().map(|d| d.code.clone()).collect();
        assert_eq!(messages.len(), 2, "{messages:?}");
        assert!(messages.contains(&Some("undefined-job".to_owned())));
    }

    #[test]
    fn is_ci_path_matches_the_icon_patterns() {
        for path in [
            ".gitlab-ci.yml",
            "/p/.gitlab-ci.yaml",
            "/p/team.gitlab-ci.yml",
            "x.gitlab-ci.yaml",
            "/p/a.gitlab-ci.deploy.yml",
            "a.gitlab-ci.b.yaml",
        ] {
            assert!(ci::is_ci_path(path), "{path}");
        }
        for path in ["/p/gitlab-ci.yml", "/p/ci.yml", "/p/.gitlab-ci.json", "/p/docker-compose.yml"] {
            assert!(!ci::is_ci_path(path), "{path}");
        }
    }

    #[test]
    fn yaml_provider_ignores_unrelated_files() {
        let mut plugin = GitLab::default();
        let doc = Document {
            path: "/project/docker-compose.yml".into(),
            language: "yaml".into(),
            text: "a:\n  extends: b\nb:\n  x: 1\n".into(),
            ..Document::default()
        };

        assert!(plugin.definition(&doc, Position::new(1, 12)).is_empty());
        let host = setup();
        host.add_file("/project/docker-compose.yml", "a:\n  needs: [ghost]\n");
        GitLab::default().handle_events(&[Event::FileSaved("/project/docker-compose.yml".into())], &host.invocation());
        assert!(host.diagnostics().get("/project/docker-compose.yml").is_none());
    }

    #[test]
    fn definition_on_extends_opens_job() {
        let yaml = ".tpl:\n  script: [x]\njob:\n  extends: .tpl\n  needs: [job2]\njob2:\n  script: [x]\n";
        let mut plugin = GitLab::default();
        let doc = Document { path: "/project/.gitlab-ci.yml".into(), language: "yaml".into(), text: yaml.into(), ..Document::default() };

        let extends = plugin.definition(&doc, Position::new(3, 14));
        let needs = plugin.definition(&doc, Position::new(4, 12));

        assert_eq!(extends.len(), 1);
        assert_eq!(extends[0].range.start, Position::new(0, 0));
        assert_eq!(needs[0].range.start, Position::new(5, 0));
        assert!(plugin.definition(&doc, Position::new(1, 3)).is_empty());
    }

    #[test]
    fn definition_on_stage_opens_stages_entry() {
        let yaml = "stages:\n  - build\n  - test\njob:\n  stage: test\n";
        let mut plugin = GitLab::default();
        let doc = Document { path: "/project/.gitlab-ci.yml".into(), language: "yaml".into(), text: yaml.into(), ..Document::default() };

        let found = plugin.definition(&doc, Position::new(4, 10));

        assert_eq!(found[0].range.start, Position::new(2, 4));
    }

    #[test]
    fn saved_file_publishes_findings_with_codes() {
        let host = setup();
        let mut plugin = GitLab::default();
        let problems = open(&host, &mut plugin, "job:\n  needs: [ghost]\n");

        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code.as_deref(), Some("undefined-job"));
        assert_eq!(problems[0].range.start, Position::new(1, 10));
    }

    #[test]
    fn fixed_file_publishes_empty_list() {
        let host = setup();
        let mut plugin = GitLab::default();
        assert_eq!(open(&host, &mut plugin, "job:\n  needs: [ghost]\n").len(), 1);

        assert!(open(&host, &mut plugin, "ghost:\n  script: [x]\njob:\n  needs: [ghost]\n").is_empty());
    }

    #[test]
    fn disabled_group_is_not_reported() {
        let host = setup();
        let mut plugin = GitLab::default();
        host.set_setting("ciJobReferences", "false");

        assert!(open(&host, &mut plugin, "job:\n  needs: [ghost]\n").is_empty());
    }
}

mod worktree_tests {
    use super::*;

    fn merge_request(branch: &str) -> MergeRequest {
        MergeRequest { iid: 7, source_branch: branch.into(), source_project_id: 5, target_project_id: 5, ..MergeRequest::default() }
    }

    #[test]
    fn worktree_args_use_the_merge_request_branch() {
        let plan = branch::worktree_plan("/work/tool", "origin", &merge_request("feature/x")).unwrap();

        assert_eq!(plan.path, "/work/tool-feature-x");
        assert_eq!(plan.fetch, ["fetch", "--", "origin", "+refs/heads/feature/x:refs/remotes/origin/feature/x"]);
        assert_eq!(plan.add, ["worktree", "add", "--detach", "--", "/work/tool-feature-x", "origin/feature/x"]);
    }

    #[test]
    fn fork_worktree_checks_out_the_fetched_merge_request_head() {
        let mut mr = merge_request("fix");
        mr.source_project_id = 9;
        mr.author.username = "ann".into();
        let plan = branch::worktree_plan("/work/tool", "origin", &mr).unwrap();

        assert_eq!(plan.fetch, ["fetch", "--", "origin", "+refs/merge-requests/7/head:refs/remotes/origin/fork/ann/fix"]);
        assert_eq!(plan.add.last().map(String::as_str), Some("origin/fork/ann/fix"));
    }

    #[test]
    fn worktree_refuses_option_like_branch_names() {
        assert!(branch::worktree_plan("/work/tool", "origin", &merge_request("--upload-pack=x")).is_err());
        assert!(branch::worktree_plan("/work/tool", "origin", &merge_request("-b")).is_err());
    }

    #[test]
    fn worktree_path_is_sanitised() {
        assert_eq!(branch::worktree_path("/work/tool/", "../../etc/passwd"), Some("/work/tool-etc-passwd".into()));
        assert_eq!(branch::worktree_path("/work/tool", "a b\\c"), Some("/work/tool-a-b-c".into()));
        assert_eq!(branch::worktree_path("/work/tool", "..."), None);
        assert_eq!(branch::worktree_path("/work/tool", "..hidden"), Some("/work/tool-hidden".into()));
    }

    #[test]
    fn worktree_command_adds_it_and_notifies_the_path() {
        let host = setup();
        host.on_process("git", git_ok);
        let _plugin = selected(&host);

        GitLab::default().run_command("open-merge-request-in-worktree", &host.invocation(), None).unwrap();

        assert_eq!(
            git_runs(&host),
            ["fetch -- origin +refs/heads/feature:refs/remotes/origin/feature", "worktree add --detach -- /project-feature origin/feature",]
        );
        assert!(host.notifications()[0].body.contains("/project-feature"));
    }

    #[test]
    fn worktree_without_git_notifies() {
        let without_git: Vec<&str> = CAPS.iter().copied().filter(|c| *c != "process:git").collect();
        let host = setup().with_capabilities(&without_git);
        let _plugin = selected(&host);

        GitLab::default().run_command("open-merge-request-in-worktree", &host.invocation(), None).unwrap();

        assert!(host.notifications()[0].body.contains("process:git permission"));
    }

    #[test]
    fn update_branch_request_targets_rebase_endpoint() {
        let host = setup();
        let _plugin = selected(&host);
        host.answer_next_prompt(Some(&[]));

        let message = GitLab::default().run_command("update-merge-request-branch", &host.invocation(), None).unwrap();

        let posted = writes(&host);
        assert_eq!(posted[0].0, "PUT");
        assert!(posted[0].1.ends_with("/projects/g%2Fp/merge_requests/7/rebase"));
        assert_eq!(message.as_deref(), Some("Rebase of !7 requested."));
    }

    #[test]
    fn leftover_merge_error_is_not_reported_as_a_new_failure() {
        let host = setup();
        host.on_fetch(|req| match (req.method.as_str(), req.url.contains("include_rebase_in_progress")) {
            ("GET", true) => ok(r#"{"rebase_in_progress": true, "merge_error": "Rebase failed. Please rebase locally"}"#),
            _ => gitlab(req),
        });
        let _plugin = selected(&host);
        host.answer_next_prompt(Some(&[]));

        let message = GitLab::default().run_command("update-merge-request-branch", &host.invocation(), None).unwrap();

        assert_eq!(message.as_deref(), Some("Rebase of !7 requested."));
    }

    #[test]
    fn update_branch_conflict_response_is_reported() {
        let host = setup();
        host.on_fetch(|req| match (req.method.as_str(), req.url.contains("include_rebase_in_progress")) {
            ("GET", true) => ok(r#"{"rebase_in_progress": false, "merge_error": "Rebase failed. Please rebase locally"}"#),
            _ => gitlab(req),
        });
        let _plugin = selected(&host);
        host.answer_next_prompt(Some(&[]));

        let error = GitLab::default().run_command("update-merge-request-branch", &host.invocation(), None).unwrap_err();

        assert_eq!(error.to_string(), "Rebase of !7 failed: Rebase failed. Please rebase locally");
    }

    #[test]
    fn rebase_command_asks_first_and_does_nothing_when_cancelled() {
        let host = setup();
        let _plugin = selected(&host);

        let message = GitLab::default().run_command("update-merge-request-branch", &host.invocation(), None).unwrap();

        assert_eq!(message, None);
        assert!(writes(&host).is_empty());
    }
}

mod create_merge_request_tests {
    use super::*;
    use serde_json::Value;

    fn git_branches(args: &[String], _: Option<&str>) -> Result<ProcessOutput, Error> {
        let stdout = match args[0].as_str() {
            "branch" => "main\nfeature\n",
            _ => "Add parser\n",
        };

        Ok(ProcessOutput { status: Some(0), stdout: stdout.into(), stderr: String::new() })
    }

    fn on_feature() -> MockHost {
        let host = setup();
        host.set_vcs_status(Some(status("feature")));
        host.on_process("git", git_branches);
        host
    }

    fn answers(host: &MockHost, answers: &[Option<&[&str]>]) {
        for answer in answers {
            host.answer_next_prompt(*answer);
        }
    }

    const ALL: [Option<&[&str]>; 5] = [Some(&["feature"]), Some(&["develop"]), Some(&["Add parser"]), Some(&["Adds it"]), Some(&["yes"])];

    fn create(host: &MockHost) {
        GitLab::default().run_command("create-merge-request", &host.invocation(), None).unwrap();
    }

    fn created(host: &MockHost) -> Vec<Value> {
        writes(host)
            .into_iter()
            .filter(|(method, url, _)| method == "POST" && url == "https://gitlab.com/api/v4/projects/g%2Fp/merge_requests")
            .map(|(_, _, body)| serde_json::from_str(&body).unwrap())
            .collect()
    }

    #[test]
    fn create_merge_request_asks_branches_title_description_and_draft() {
        let host = on_feature();
        answers(&host, &ALL);

        create(&host);

        let posts = created(&host);
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0]["source_branch"], "feature");
        assert_eq!(posts[0]["target_branch"], "develop");
        assert_eq!(posts[0]["title"], "Draft: Add parser");
        assert_eq!(posts[0]["description"], "Adds it");
        assert_eq!(host.notifications()[0].title, "Merge request created");
    }

    #[test]
    fn a_title_already_marked_draft_is_not_prefixed_twice_and_no_draft_keeps_it_plain() {
        let host = on_feature();
        answers(&host, &[ALL[0], ALL[1], Some(&["Draft: wip"]), ALL[3], ALL[4]]);
        create(&host);
        let host2 = on_feature();
        answers(&host2, &[ALL[0], ALL[1], ALL[2], ALL[3], None]);
        create(&host2);

        assert_eq!(created(&host)[0]["title"], "Draft: wip");
        assert_eq!(created(&host2)[0]["title"], "Add parser");
    }

    #[test]
    fn dismissing_any_prompt_creates_nothing() {
        for dismissed in 0..4 {
            let host = on_feature();
            let mut all = ALL;
            all[dismissed] = None;
            answers(&host, &all[..=dismissed]);

            create(&host);

            assert!(writes(&host).is_empty(), "prompt {dismissed}");
            assert!(host.notifications().is_empty(), "prompt {dismissed}");
        }
    }

    #[test]
    fn the_branch_prompts_start_with_the_current_and_the_default_branch() {
        let host = on_feature();
        host.answer_next_prompt(Some(&["develop"]));

        let mut plugin = GitLab::default();
        let repo = gitlab_project(&host.invocation()).unwrap();
        let defaults = plugin.merge_request_defaults(&host.invocation(), &repo).unwrap();

        assert_eq!(defaults.sources, ["feature", "main"]);
        assert_eq!(defaults.targets, ["main", "develop", "feature"]);
        assert_eq!(defaults.title, "Add parser");
    }

    #[test]
    fn api_failure_notifies_and_offers_to_open_gitlab() {
        let host = on_feature();
        host.on_fetch(|req| match req.method.as_str() {
            "POST" => Ok(HttpResponse { status: 409, headers: vec![], body: b"{}".to_vec() }),
            _ => gitlab(req),
        });
        answers(&host, &ALL);
        host.answer_next_prompt(Some(&["yes"]));

        create(&host);

        assert_eq!(host.notifications()[0].title, "Merge request not created");
        assert_eq!(host.opened_urls(), ["https://gitlab.com/g/p/-/merge_requests/new?merge_request%5Bsource_branch%5D=feature"]);
    }

    #[test]
    fn declining_open_on_gitlab_after_a_failure_opens_nothing() {
        let host = on_feature();
        host.on_fetch(|req| match req.method.as_str() {
            "POST" => Ok(HttpResponse { status: 409, headers: vec![], body: b"{}".to_vec() }),
            _ => gitlab(req),
        });
        answers(&host, &ALL);
        host.answer_next_prompt(None);

        create(&host);

        assert!(host.opened_urls().is_empty());
    }

    #[test]
    fn an_empty_title_creates_nothing() {
        let host = on_feature();
        answers(&host, &[ALL[0], ALL[1], Some(&["  "]), ALL[3], ALL[4]]);

        let result = GitLab::default().run_command("create-merge-request", &host.invocation(), None);

        assert!(result.is_err());
        assert!(created(&host).is_empty());
    }
}

mod snippet_prompt_tests {
    use super::*;
    use serde_json::Value;

    fn with_file() -> MockHost {
        let host = setup();
        host.open_editor("/project/src/a.rs", "rust", "fn main() {}\n", &[(0, 3, 0, 7)]);
        host.add_file("docs/b.md", "second\n");
        host
    }

    fn answers(host: &MockHost, answers: &[Option<&[&str]>]) {
        for answer in answers {
            host.answer_next_prompt(*answer);
        }
    }

    const ALL: [Option<&[&str]>; 5] = [Some(&["Mine"]), Some(&["internal"]), Some(&["docs/b.md"]), Some(&["docs/b.md"]), Some(&[""])];

    fn create(host: &MockHost) {
        GitLab::default().run_command("create-snippet", &host.invocation(), None).unwrap();
    }

    fn posted(host: &MockHost) -> Vec<Value> {
        writes(host).into_iter().map(|(_, _, body)| serde_json::from_str(&body).unwrap()).collect()
    }

    #[test]
    fn snippet_prompts_collect_title_visibility_and_extra_files() {
        let host = with_file();
        answers(&host, &ALL);

        create(&host);

        let posted = posted(&host);
        assert_eq!(posted.len(), 1);
        assert_eq!(posted[0]["title"], "Mine");
        assert_eq!(posted[0]["visibility"], "internal");
        assert_eq!(posted[0]["files"][0]["file_path"], "src/a.rs");
        assert_eq!(posted[0]["files"][0]["content"], "main");
        assert_eq!(posted[0]["files"][1]["file_path"], "docs/b.md");
        assert_eq!(posted[0]["files"][1]["content"], "second\n");
        assert_eq!(posted[0]["files"].as_array().unwrap().len(), 2);
        assert_eq!(host.notifications()[0].title, "Snippet created");
    }

    #[test]
    fn an_empty_title_names_the_snippet_after_the_file() {
        let host = with_file();
        answers(&host, &[Some(&[""]), Some(&["private"]), Some(&[""])]);

        create(&host);

        assert_eq!(posted(&host)[0]["title"], "a.rs");
    }

    #[test]
    fn public_snippet_needs_the_confirmation() {
        let declined = with_file();
        answers(&declined, &[ALL[0], Some(&["public"]), None]);
        create(&declined);

        let confirmed = with_file();
        answers(&confirmed, &[ALL[0], Some(&["public"]), Some(&["yes"]), Some(&[""])]);
        create(&confirmed);

        assert!(writes(&declined).is_empty());
        assert_eq!(posted(&confirmed)[0]["visibility"], "public");
    }

    #[test]
    fn dismissing_any_snippet_prompt_creates_nothing() {
        for dismissed in 0..3 {
            let host = with_file();
            let mut all = ALL;
            all[dismissed] = None;
            answers(&host, &all[..=dismissed]);

            create(&host);

            assert!(writes(&host).is_empty(), "prompt {dismissed}");
            assert!(host.notifications().is_empty(), "prompt {dismissed}");
        }
    }

    #[test]
    fn an_unreadable_or_unsafe_extra_path_creates_nothing() {
        for path in ["missing.md", "../secret", "/etc/passwd"] {
            let host = with_file();
            answers(&host, &[ALL[0], ALL[1], Some(&[path]), Some(&[""])]);

            let result = GitLab::default().run_command("create-snippet", &host.invocation(), None);

            assert!(result.is_err(), "{path}");
            assert!(writes(&host).is_empty(), "{path}");
        }
    }

    #[test]
    fn a_stale_forms_setting_is_ignored() {
        let host = with_file();
        host.set_setting("forms", "false");
        answers(&host, &[ALL[0], Some(&["private"]), Some(&[""])]);

        create(&host);

        assert_eq!(posted(&host).len(), 1);
    }
}

mod share_prompt_tests {
    use super::*;
    use serde_json::Value;
    use std::cell::Cell;

    fn git_staging(
        statuses: Vec<&'static str>,
        staged: &'static str,
        failing: &'static str,
    ) -> impl Fn(&[String], Option<&str>) -> Result<ProcessOutput, Error> {
        let calls = Cell::new(0);
        move |args, _| {
            let stdout = match args[0].as_str() {
                "status" => {
                    let files = statuses[calls.get().min(statuses.len() - 1)];
                    calls.set(calls.get() + 1);
                    files.replace('\n', "\0")
                }
                "diff" => staged.to_owned(),
                "symbolic-ref" => "main\n".into(),
                _ => String::new(),
            };
            let status = if args[0] == failing { 1 } else { 0 };
            Ok(ProcessOutput { status: Some(status), stdout, stderr: "boom".into() })
        }
    }

    const A: &str = "?? src/a.rs\n M README.md\n";
    const A_STAGED: &str = "src/a.rs\0README.md\0";

    fn unshared() -> MockHost {
        let host = setup();
        host.set_vcs_remotes(&[]);
        host.on_process("git", git_ok);
        host
    }

    fn uncommitted(process: impl Fn(&[String], Option<&str>) -> Result<ProcessOutput, Error> + 'static) -> MockHost {
        let host = unshared();
        host.set_vcs_status(Some(RepositoryStatus { commit: None, ..status("main") }));
        host.on_process("git", process);
        host
    }

    fn answers(host: &MockHost, answers: &[Option<&[&str]>]) {
        for answer in answers {
            host.answer_next_prompt(*answer);
        }
    }

    const DEFAULTS: [Option<&[&str]>; 2] = [Some(&[""]), Some(&["private"])];
    const YES: Option<&[&str]> = Some(&["yes"]);

    fn share(host: &MockHost) -> Result<Option<String>, Error> {
        GitLab::default().run_command("share-project", &host.invocation(), None)
    }

    fn created(host: &MockHost) -> Vec<Value> {
        writes(host)
            .into_iter()
            .filter(|(_, url, _)| url == "https://gitlab.com/api/v4/projects")
            .map(|(_, _, body)| serde_json::from_str(&body).unwrap())
            .collect()
    }

    fn nothing_happened(host: &MockHost) {
        assert!(writes(host).is_empty(), "{:?}", writes(host));
        assert!(host.process_runs().is_empty(), "{:?}", git_runs(host));
        assert!(host.terminals().is_empty());
    }

    #[test]
    fn share_prompts_collect_name_and_visibility() {
        let host = unshared();
        answers(&host, &[Some(&["tool"]), Some(&["internal"])]);

        share(&host).unwrap();

        assert_eq!(created(&host), [serde_json::json!({"name": "tool", "visibility": "internal"})]);
        assert_eq!(git_runs(&host), ["--version", "remote add -- origin https://gitlab.com/me/project.git"]);
        assert_eq!(host.terminals()[0].args, ["push", "-u", "--", "origin", "main"]);
        assert_eq!(host.notifications()[0].body, "https://gitlab.com/me/project");
    }

    #[test]
    fn an_empty_name_is_the_folder_name() {
        let host = unshared();
        answers(&host, &DEFAULTS);

        share(&host).unwrap();

        assert_eq!(created(&host), [serde_json::json!({"name": "project", "visibility": "private"})]);
    }

    #[test]
    fn a_public_project_needs_the_confirmation() {
        let declined = unshared();
        answers(&declined, &[DEFAULTS[0], Some(&["public"]), None]);
        share(&declined).unwrap();

        let confirmed = unshared();
        answers(&confirmed, &[DEFAULTS[0], Some(&["public"]), YES]);
        share(&confirmed).unwrap();

        nothing_happened(&declined);
        assert_eq!(created(&confirmed)[0]["visibility"], "public");
    }

    #[test]
    fn dismissing_any_share_prompt_creates_nothing() {
        for dismissed in 0..2 {
            let host = unshared();
            let mut all = DEFAULTS;
            all[dismissed] = None;
            answers(&host, &all[..=dismissed]);

            share(&host).unwrap();

            nothing_happened(&host);
        }
    }

    #[test]
    fn a_branch_with_shell_metacharacters_is_refused_before_pushing() {
        let host = unshared();
        host.set_vcs_status(Some(status("x;rm -rf ~")));
        answers(&host, &DEFAULTS);

        assert!(share(&host).is_err());

        nothing_happened(&host);
    }

    #[test]
    fn a_returned_clone_url_on_another_host_is_refused_and_not_added() {
        let host = unshared();
        host.on_fetch(|r| match r.method == "POST" && r.url.ends_with("/api/v4/projects") {
            true => ok(r#"{"web_url": "https://gitlab.com/me/project", "http_url_to_repo": "https://evil.test/x.git", "ssh_url_to_repo": "git@evil.test:x.git"}"#),
            false => gitlab(r),
        });
        answers(&host, &DEFAULTS);

        let result = share(&host);

        assert!(result.unwrap_err().to_string().contains("refusing the clone URL"));
        assert_eq!(git_runs(&host), ["--version"]);
        assert!(host.terminals().is_empty());
    }

    #[test]
    fn a_repository_without_a_commit_commits_after_two_confirms() {
        let host = uncommitted(git_staging(vec![A], A_STAGED, ""));
        answers(&host, &[DEFAULTS[0], DEFAULTS[1], YES, YES]);

        share(&host).unwrap();

        assert_eq!(
            git_runs(&host),
            [
                "--version",
                "status --porcelain -z --untracked-files=all",
                "--version",
                "status --porcelain -z --untracked-files=all",
                "add -A",
                "diff --cached --name-only -z --no-renames",
                "commit -m Initial commit",
                "remote add -- origin https://gitlab.com/me/project.git"
            ]
        );
        assert_eq!(created(&host).len(), 1);
        assert_eq!(host.terminals().len(), 1);
    }

    #[test]
    fn declining_either_commit_confirm_leaves_everything_alone() {
        for confirms in [vec![None], vec![YES, None]] {
            let host = uncommitted(git_staging(vec![A], A_STAGED, ""));
            let mut all = DEFAULTS.to_vec();
            all.extend(confirms);
            answers(&host, &all);

            share(&host).unwrap();

            let runs = git_runs(&host);
            assert!(!runs.iter().any(|r| r.starts_with("add") || r.starts_with("commit") || r.starts_with("remote")), "{runs:?}");
            assert!(writes(&host).is_empty());
            assert!(host.terminals().is_empty());
        }
    }

    #[test]
    fn a_file_staged_after_the_confirm_is_unstaged_and_listed_again() {
        let host = uncommitted(git_staging(vec![A], "src/a.rs\0README.md\0notes.txt\0", ""));
        answers(&host, &[DEFAULTS[0], DEFAULTS[1], YES, YES, None]);

        share(&host).unwrap();

        let runs = git_runs(&host);
        assert!(runs.contains(&"reset -q".to_owned()), "{runs:?}");
        assert!(!runs.iter().any(|r| r.starts_with("commit") || r.starts_with("remote")), "{runs:?}");
        assert!(created(&host).is_empty());
        assert!(host.terminals().is_empty());
    }

    #[test]
    fn the_commit_confirm_is_bound_to_the_files_it_listed() {
        let host = uncommitted(git_staging(vec!["?? src/a.rs\n", "?? src/a.rs\n?? notes.txt\n"], A_STAGED, ""));
        answers(&host, &[DEFAULTS[0], DEFAULTS[1], YES, YES, None]);

        share(&host).unwrap();

        assert!(!git_runs(&host).iter().any(|r| r.starts_with("add")));
        assert!(writes(&host).is_empty());
    }

    #[test]
    fn porcelain_renames_list_both_sides_and_order_does_not_matter_for_the_confirm() {
        let host = uncommitted(git_staging(vec!["R  new.rs\nold.rs\n?? b.rs\n"], "b.rs\0new.rs\0old.rs\0", ""));
        answers(&host, &[DEFAULTS[0], DEFAULTS[1], YES, YES]);

        share(&host).unwrap();

        assert!(git_runs(&host).contains(&"commit -m Initial commit".to_owned()));
        assert_eq!(created(&host).len(), 1);
    }

    #[test]
    fn commit_all_refuses_secret_looking_files() {
        for file in [".env", "deploy.key", "id_rsa", "config/.env.local", "certs/site.PEM", ".ssh/id_ed25519.pub"] {
            let listing: &'static str = Box::leak(format!("?? src/a.rs\n?? {file}\n").into_boxed_str());
            let host = uncommitted(git_staging(vec![listing], A_STAGED, ""));
            answers(&host, &[DEFAULTS[0], DEFAULTS[1], YES]);

            let error = share(&host).unwrap_err();

            assert!(!git_runs(&host).iter().any(|r| r.starts_with("add")), "{file}");
            assert!(writes(&host).is_empty(), "{file}");
            assert!(error.to_string().contains(file), "{file}");
        }
    }

    #[test]
    fn init_runs_only_when_there_is_no_repository() {
        let host = unshared();
        host.set_vcs_status(None);
        host.on_process("git", git_staging(vec!["?? src/a.rs\n"], "src/a.rs\0", ""));
        answers(&host, &[DEFAULTS[0], DEFAULTS[1], YES, YES]);

        share(&host).unwrap();

        assert_eq!(git_runs(&host)[..3], ["--version", "init", "symbolic-ref --short HEAD"]);
        assert_eq!(created(&host).len(), 1);
    }

    #[test]
    fn nothing_is_created_when_git_init_or_commit_fails() {
        let host = unshared();
        host.set_vcs_status(None);
        host.on_process("git", git_staging(vec!["?? src/a.rs\n"], "src/a.rs\0", "init"));
        answers(&host, &[DEFAULTS[0], DEFAULTS[1], YES]);
        let error = share(&host).unwrap_err();
        assert!(writes(&host).is_empty());
        assert!(error.to_string().contains("Git init failed"));

        let host = uncommitted(git_staging(vec![A], A_STAGED, "commit"));
        answers(&host, &[DEFAULTS[0], DEFAULTS[1], YES, YES]);
        assert!(share(&host).is_err());
        assert!(writes(&host).is_empty());
        assert!(host.terminals().is_empty());
    }

    #[test]
    fn an_existing_gitlab_remote_refuses() {
        let host = setup();
        host.on_process("git", git_ok);
        answers(&host, &DEFAULTS);

        let error = share(&host).unwrap_err();

        nothing_happened(&host);
        assert!(error.to_string().contains("already has a remote on the configured GitLab server"));
    }

    #[test]
    fn share_without_process_git_names_the_capability() {
        let without_git: Vec<&str> = CAPS.iter().copied().filter(|c| *c != "process:git").collect();
        let host = unshared().with_capabilities(&without_git);
        answers(&host, &DEFAULTS);

        let result = share(&host);

        assert_eq!(result, Err(Error::Denied("process:git".into())));
        assert!(writes(&host).is_empty());
    }
}

mod note_action_tests {
    use super::*;

    const MR_URL: &str = "https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7";

    const MINE: &str = r#"[
        {"id": "d1", "notes": [
            {"id": 11, "body": "Why?", "author": {"id": 3, "username": "bob"}, "created_at": "2026-01-02", "resolvable": true},
            {"id": 12, "body": "Because", "author": {"id": 1, "username": "me"}, "created_at": "2026-01-03"},
            {"id": 14, "body": "Also", "author": {"id": 1, "username": "me"}, "created_at": "2026-01-04"}
        ]},
        {"id": "d2", "notes": [{"id": 13, "body": "changed the description", "author": {"id": 1, "username": "me"}, "created_at": "2026-01-01", "system": true}]}
    ]"#;

    fn notes(req: &HttpRequest) -> Result<HttpResponse, Error> {
        let path = req.url.strip_prefix(MR_URL).unwrap_or("").split('?').next().unwrap_or("");

        match (req.method.as_str(), path) {
            ("GET", "/discussions") => ok(MINE),
            ("GET", "/notes/12/award_emoji") => ok(r#"[{"id": 55, "name": "thumbsup", "user": {"id": 1, "username": "me"}},
                    {"id": 56, "name": "rocket", "user": {"id": 3, "username": "bob"}}]"#),
            ("GET", p) if p.ends_with("/award_emoji") => ok("[]"),
            ("DELETE", _) => ok(""),
            _ => gitlab(req),
        }
    }

    fn host() -> MockHost {
        let host = setup();
        host.on_fetch(notes);
        host
    }

    fn on(host: &MockHost, key: &str) -> GitLab {
        let mut plugin = GitLab::default();
        plugin.handle(LIST, ViewEvent::Select(key.into()), &host.invocation()).unwrap();
        plugin.render(DETAILS, None, &host.invocation()).unwrap();
        plugin
    }

    fn click(plugin: &mut GitLab, host: &MockHost, id: &str) -> View {
        plugin.handle(DETAILS, ViewEvent::Button(id.into()), &host.invocation()).unwrap();
        plugin.render(DETAILS, None, &host.invocation()).unwrap()
    }

    fn comment(view: &View) -> &FieldValue {
        let Body::Form(fields) = &view.body else { panic!("{view:?}") };
        &fields.iter().find(|f| f.key == COMMENT_KEY).unwrap().value
    }

    #[test]
    fn edit_saves_the_comment_text_to_the_selected_own_note() {
        let host = host();
        let mut plugin = on(&host, "n:7:d1:12");
        let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();
        assert_eq!(label(&view, "edit-note"), "Save Edit");

        plugin.handle(DETAILS, field("comment", "Because of X"), &host.invocation()).unwrap();
        let view = click(&mut plugin, &host, "edit-note");

        assert_eq!(writes(&host), [("PUT".into(), format!("{MR_URL}/notes/12"), r#"{"body":"Because of X"}"#.into())]);
        assert!(view.status.as_deref().unwrap().starts_with("Comment edited."), "{view:?}");
        assert_eq!(host.storage().get("comment"), None);
    }

    #[test]
    fn edit_and_delete_are_not_offered_for_someone_elses_note_or_a_system_note() {
        for key in ["d:7:d1", "n:7:d1:11", "d:7:d2"] {
            let host = host();
            let mut plugin = on(&host, key);
            plugin.handle(DETAILS, field("comment", "taken over"), &host.invocation()).unwrap();

            let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();
            assert!(!buttons(&view).contains(&"edit-note") && !buttons(&view).contains(&"delete-note"), "{key}: {view:?}");

            click(&mut plugin, &host, "edit-note");
            click(&mut plugin, &host, "delete-note");
            click(&mut plugin, &host, "delete-note");
            assert!(writes(&host).is_empty(), "{key}");
        }
    }

    #[test]
    fn delete_needs_a_second_click() {
        let host = host();
        let mut plugin = on(&host, "n:7:d1:12");
        let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();
        assert_eq!(label(&view, "delete-note"), "Delete Comment");

        let view = click(&mut plugin, &host, "delete-note");
        assert!(writes(&host).is_empty(), "the first click only asks to confirm");
        assert_eq!(label(&view, "delete-note"), "Confirm Delete Comment");

        let view = click(&mut plugin, &host, "delete-note");

        assert_eq!(writes(&host), [("DELETE".into(), format!("{MR_URL}/notes/12"), String::new())]);
        assert!(view.status.as_deref().unwrap().starts_with("Comment deleted."), "{view:?}");
    }

    #[test]
    fn another_action_cancels_a_pending_delete() {
        let host = host();
        let mut plugin = on(&host, "n:7:d1:12");

        click(&mut plugin, &host, "delete-note");
        plugin.handle(LIST, ViewEvent::Select("n:7:d1:14".into()), &host.invocation()).unwrap();
        let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();
        assert_eq!(label(&view, "delete-note"), "Delete Comment");

        click(&mut plugin, &host, "delete-note");
        click(&mut plugin, &host, "copy");
        let view = click(&mut plugin, &host, "delete-note");
        assert!(writes(&host).is_empty());
        assert_eq!(label(&view, "delete-note"), "Confirm Delete Comment");

        storage::set("note", "12").unwrap();
        click(&mut plugin, &host, "delete-note");
        assert!(writes(&host).is_empty(), "a confirm shown for note 14 does not delete note 12");

        click(&mut plugin, &host, "delete-note");
        assert_eq!(writes(&host), [("DELETE".into(), format!("{MR_URL}/notes/12"), String::new())]);
    }

    #[test]
    fn react_adds_the_chosen_emoji_to_the_selected_note() {
        let host = host();
        let mut plugin = on(&host, "n:7:d1:12");

        plugin.handle(DETAILS, field("emoji", "rocket"), &host.invocation()).unwrap();
        let view = click(&mut plugin, &host, "react");

        assert_eq!(writes(&host), [("POST".into(), format!("{MR_URL}/notes/12/award_emoji?name=rocket"), String::new())]);
        assert!(view.status.as_deref().unwrap().starts_with("Reacted with :rocket:."), "{view:?}");
    }

    #[test]
    fn react_again_removes_my_own_award() {
        let host = host();
        let mut plugin = on(&host, "n:7:d1:12");

        let view = click(&mut plugin, &host, "react");

        assert_eq!(writes(&host), [("DELETE".into(), format!("{MR_URL}/notes/12/award_emoji/55"), String::new())]);
        assert!(view.status.as_deref().unwrap().starts_with("Removed your :thumbsup:."), "{view:?}");
    }

    #[test]
    fn a_user_without_an_id_owns_no_note_and_no_award() {
        let host = setup();
        host.on_fetch(|r| {
            let path = r.url.strip_prefix(MR_URL).unwrap_or("").split('?').next().unwrap_or("");
            match (r.method.as_str(), path) {
                _ if r.url.ends_with("/api/v4/user") => ok(r#"{"username": "me"}"#),
                ("GET", "/discussions") => {
                    ok(r#"[{"id": "d1", "notes": [{"id": 12, "body": "x", "author": {"username": "me"}, "created_at": "2026-01-03"}]}]"#)
                }
                ("GET", "/notes/12/award_emoji") => ok(r#"[{"id": 55, "name": "thumbsup", "user": {"username": "me"}}]"#),
                _ => notes(r),
            }
        });
        let mut plugin = on(&host, "n:7:d1:12");

        let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();
        assert!(!buttons(&view).contains(&"edit-note") && !buttons(&view).contains(&"delete-note"), "{view:?}");
        click(&mut plugin, &host, "edit-note");
        click(&mut plugin, &host, "delete-note");
        click(&mut plugin, &host, "delete-note");
        assert!(writes(&host).is_empty(), "{:?}", writes(&host));

        click(&mut plugin, &host, "react");

        assert_eq!(writes(&host), [("POST".into(), format!("{MR_URL}/notes/12/award_emoji?name=thumbsup"), String::new())]);
    }

    #[test]
    fn react_without_a_selected_note_awards_the_merge_request() {
        let host = host();
        let mut plugin = on(&host, "mr:7");

        plugin.handle(DETAILS, field("emoji", "tada"), &host.invocation()).unwrap();
        click(&mut plugin, &host, "react");

        assert_eq!(writes(&host), [("POST".into(), format!("{MR_URL}/award_emoji?name=tada"), String::new())]);
    }

    #[test]
    fn an_emoji_outside_the_list_is_refused_and_no_request_is_sent() {
        for emoji in ["thumbsup&name=x", "../x"] {
            let host = host();
            let mut plugin = on(&host, "n:7:d1:12");
            let before = host.requests().len();

            plugin.handle(DETAILS, field("emoji", emoji), &host.invocation()).unwrap();
            plugin.handle(DETAILS, ViewEvent::Button("react".into()), &host.invocation()).unwrap();

            assert_eq!(host.requests().len(), before, "{emoji}");
            assert!(plugin.status.as_deref().unwrap().contains("refusing the emoji"), "{emoji}");

            let api = api::Project("g/p");
            assert!(api.add_award(7, Some(12), emoji).is_err());
            assert_eq!(host.requests().len(), before, "{emoji}");
        }
    }

    #[test]
    fn note_ids_come_from_the_tree_key_not_from_form_text() {
        let host = host();
        let mut plugin = on(&host, "n:7:d1:12");

        for key in ["note", "note-id", "note_id", "id"] {
            plugin.handle(DETAILS, field(key, "14"), &host.invocation()).unwrap();
        }
        plugin.handle(DETAILS, field("comment", "14"), &host.invocation()).unwrap();
        click(&mut plugin, &host, "edit-note");
        click(&mut plugin, &host, "delete-note");
        click(&mut plugin, &host, "delete-note");

        let urls: Vec<String> = writes(&host).into_iter().map(|w| w.1).collect();
        assert_eq!(urls, [format!("{MR_URL}/notes/12"), format!("{MR_URL}/notes/12")]);
    }

    #[test]
    fn a_failed_edit_keeps_the_comment_text_and_says_why() {
        let host = host();
        host.on_fetch(|r| match r.method == "PUT" {
            true => Ok(HttpResponse { status: 400, headers: vec![], body: br#"{"message":"note is too long"}"#.to_vec() }),
            false => notes(r),
        });
        let mut plugin = on(&host, "n:7:d1:12");

        plugin.handle(DETAILS, field("comment", "LGTM"), &host.invocation()).unwrap();
        let view = click(&mut plugin, &host, "edit-note");

        assert_eq!(host.storage().get("comment").map(String::as_str), Some("LGTM"));
        assert!(view.status.as_deref().unwrap().contains("note is too long"), "{view:?}");
        assert_eq!(comment(&view), &FieldValue::Multiline("LGTM".into()));
    }

    #[test]
    fn note_actions_are_quiet_when_the_setting_is_off() {
        let host = host();
        host.set_setting("noteActions", "false");
        let mut plugin = on(&host, "n:7:d1:12");

        let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();
        let Body::Form(fields) = &view.body else { panic!("{view:?}") };
        assert!(!fields.iter().any(|f| f.key == "emoji"), "{view:?}");
        assert!(buttons(&view).iter().all(|b| !["edit-note", "delete-note", "react"].contains(b)), "{view:?}");

        plugin.handle(DETAILS, field("comment", "x"), &host.invocation()).unwrap();
        for id in ["edit-note", "delete-note", "delete-note", "react"] {
            click(&mut plugin, &host, id);
        }

        assert!(writes(&host).is_empty());
        assert!(host.requests().iter().all(|r| !r.url.contains("award_emoji")));
        assert_eq!(plugin.status, None);
    }

    #[test]
    fn an_empty_edit_is_refused() {
        let host = host();
        let mut plugin = on(&host, "n:7:d1:12");

        plugin.handle(DETAILS, field("comment", " \n "), &host.invocation()).unwrap();
        let view = click(&mut plugin, &host, "edit-note");

        assert!(writes(&host).is_empty());
        assert!(view.status.as_deref().unwrap().starts_with("Write the new text"), "{view:?}");
    }
}

mod commit_tests {
    use super::*;

    const SHA1: &str = "aaaaaaa1111111111111111111111111111111aa";
    const SHA2: &str = "bbbbbbb2222222222222222222222222222222bb";
    const MR_URL: &str = "https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7";
    const REPO_URL: &str = "https://gitlab.com/api/v4/projects/g%2Fp/repository";

    fn commits(req: &HttpRequest) -> Result<HttpResponse, Error> {
        let url = req.url.split('?').next().unwrap_or("");

        match url {
            u if u == format!("{MR_URL}/commits") => ok(&format!(
                r#"[{{"id": "{SHA2}", "short_id": "bbbbbbb2", "title": "Second <b>", "author_name": "Ann"}},
                    {{"id": "{SHA1}", "short_id": "aaaaaaa1", "title": "First", "author_name": "Bob"}}]"#
            )),
            u if u == format!("{REPO_URL}/commits/{SHA1}/diff") => ok(
                r#"[{"old_path": "src/b.rs", "new_path": "src/b.rs", "diff": "@@ -1,2 +1,2 @@\r\n é\r\n-ü😀<script>\r\n+ü😀x</td>\r\n z\r\n"}]"#,
            ),
            _ => gitlab(req),
        }
    }

    fn host() -> MockHost {
        let host = setup();
        host.on_fetch(commits);
        host
    }

    fn on_mr(host: &MockHost) -> GitLab {
        let mut plugin = selected(host);
        render_in(&mut plugin, Section::Commits, &host);
        plugin
    }

    fn pick(plugin: &mut GitLab, host: &MockHost, key: &str) {
        handle_in(plugin, Section::Commits, ViewEvent::Activate(key.into()), &host);
    }

    fn commit_requests(host: &MockHost) -> Vec<String> {
        host.requests().into_iter().map(|r| r.url).filter(|u| u.contains("/repository/commits/")).collect()
    }

    #[test]
    fn lists_the_commits_of_the_selected_merge_request() {
        let host = host();
        let mut plugin = selected(&host);

        let view = render_in(&mut plugin, Section::Commits, &host);

        let Body::Tree(items) = &view.body else { panic!("{view:?}") };
        let rows: Vec<(&str, &str)> = items.iter().map(|i| (i.key.as_str(), i.label.as_str())).collect();
        assert_eq!(rows, [(SHA2, "bbbbbbb2 Second <b>"), (SHA1, "aaaaaaa1 First")]);
        assert_eq!(items[1].detail.as_deref(), Some("Bob"));
        assert!(host.requests().iter().any(|r| r.url.starts_with(&format!("{MR_URL}/commits?per_page="))));
    }

    #[test]
    fn activating_a_commit_shows_its_diff_in_the_changes_view_without_comment_threads() {
        let host = host();
        let mut plugin = on_mr(&host);
        assert!(changes_html(&mut plugin, &host).contains("Why?"), "the merge request diff shows its threads");

        pick(&mut plugin, &host, SHA1);

        assert_eq!(plugin.commit.as_deref(), Some(SHA1));
        assert_eq!(storage::get(sections::SECTION_KEY).unwrap().as_deref(), Some("changes"));
        assert!(host.refreshed_views().contains(&LIST.to_owned()));
        let html = changes_html(&mut plugin, &host);
        assert!(html.contains("src/b.rs") && !html.contains("src/a.rs"), "{html}");
        assert!(!html.contains("Why?") && !html.contains("data-reply"), "no comment threads in commit mode");
        assert!(html.contains("aaaaaaa1"), "{html}");
        assert_eq!(commit_requests(&host), [format!("{REPO_URL}/commits/{SHA1}/diff?per_page=100&page=1")]);

        let comment = r#"{"type":"comment","oldPath":"src/b.rs","newPath":"src/b.rs","old":null,"new":2,"body":"x","draft":false}"#;
        changes_message(&mut plugin, &host, ViewEvent::Message(comment.into())).unwrap();
        assert!(writes(&host).is_empty(), "commit diffs are read-only");
        assert!(host.view_messages().iter().any(|(view, _)| view == LIST), "the error goes to the merge request view");
    }

    #[test]
    fn all_changes_returns_to_the_merge_request_diff() {
        let host = host();
        let mut plugin = on_mr(&host);
        pick(&mut plugin, &host, SHA1);
        let view = render_in(&mut plugin, Section::Commits, &host);
        assert!(buttons(&view).contains(&"all-changes"), "{view:?}");

        handle_in(&mut plugin, Section::Commits, ViewEvent::Button("all-changes".into()), &host);

        assert_eq!(plugin.commit, None);
        let html = changes_html(&mut plugin, &host);
        assert!(html.contains("src/a.rs") && html.contains("Why?"), "{html}");
    }

    #[test]
    fn selecting_another_merge_request_clears_the_commit() {
        let host = host();
        let mut plugin = on_mr(&host);
        pick(&mut plugin, &host, SHA1);

        plugin.handle(LIST, ViewEvent::Select("mr:8".into()), &host.invocation()).unwrap();

        assert_eq!(plugin.commit, None);
        assert!(host.refreshed_views().contains(&LIST.to_owned()));
    }

    #[test]
    fn a_non_hex_commit_key_sends_no_request() {
        let host = setup();
        host.on_fetch(|req| match req.url.contains("/merge_requests/7/commits") {
            true => ok(r#"[{"id": "abc/../x", "short_id": "a"}, {"id": "HEAD", "short_id": "h"}, {"id": "abcdef1?x", "short_id": "q"}]"#),
            false => gitlab(req),
        });
        let mut plugin = on_mr(&host);
        let before = host.requests().len();

        for key in ["abc/../x", "HEAD", "abcdef1?x"] {
            pick(&mut plugin, &host, key);
            handle_in(&mut plugin, Section::Commits, ViewEvent::Select(key.into()), &host);
            assert_eq!(plugin.commit, None, "{key}");
            changes_html(&mut plugin, &host);
        }

        assert!(commit_requests(&host).is_empty(), "{:?}", commit_requests(&host));
        assert!(host.requests()[before..].iter().all(|r| !r.url.contains("..") && !r.url.contains("HEAD")));
    }

    #[test]
    fn an_empty_commit_list_shows_a_message_not_a_blank_list() {
        let host = setup();
        host.on_fetch(|req| match req.url.contains("/merge_requests/7/commits") {
            true => ok("[]"),
            false => gitlab(req),
        });
        let mut plugin = selected(&host);

        let view = render_in(&mut plugin, Section::Commits, &host);

        assert_eq!(view.body, Body::Message("!7 has no commits.".into()));
    }

    #[test]
    fn the_view_is_quiet_without_a_selected_merge_request() {
        let host = host();
        let mut plugin = GitLab::default();

        let view = render_in(&mut plugin, Section::Commits, &host);
        handle_in(&mut plugin, Section::Commits, ViewEvent::Activate(SHA1.into()), &host);

        assert!(matches!(view.body, Body::Message(_)), "{view:?}");
        assert_eq!(plugin.commit, None);
        assert!(host.requests().is_empty());
    }

    #[test]
    fn the_view_is_quiet_when_the_setting_is_off() {
        let host = host();
        let mut plugin = on_mr(&host);
        pick(&mut plugin, &host, SHA1);
        host.set_setting("commitsView", "false");
        let before = host.requests().len();

        let view = render_in(&mut plugin, Section::Commits, &host);
        pick(&mut plugin, &host, SHA2);
        let html = changes_html(&mut plugin, &host);

        assert!(matches!(view.body, Body::Message(_)), "{view:?}");
        assert!(html.contains("src/a.rs") && html.contains("Why?"), "the merge request diff, not the commit");
        assert!(host.requests()[before..].iter().all(|r| !r.url.contains("commits")));
    }

    #[test]
    fn diff_text_with_html_is_escaped_in_commit_mode() {
        let host = host();
        let mut plugin = on_mr(&host);
        pick(&mut plugin, &host, SHA1);

        let html = changes_html(&mut plugin, &host);

        assert!(html.contains("<td class=\"code\">ü😀&lt;script&gt;</td>"), "{html}");
        assert!(html.contains("<td class=\"code\">ü😀x&lt;/td&gt;</td>"), "{html}");
        let view = render_in(&mut plugin, Section::Commits, &host);
        assert!(view.status.as_deref().unwrap_or("").contains("aaaaaaa1"), "{view:?}");
    }

    #[test]
    fn crlf_and_multibyte_diff_lines_keep_their_numbers() {
        let lines = review::parse_diff("@@ -1,2 +1,2 @@\r\n é\r\n-ü😀\r\n+ü😀x\r\n z\r\n");

        let numbers: Vec<(char, Option<u32>, Option<u32>, &str)> = lines.iter().map(|l| (l.kind, l.old, l.new, l.text.as_str())).collect();
        assert_eq!(
            numbers,
            [
                ('@', None, None, "@@ -1,2 +1,2 @@"),
                (' ', Some(1), Some(1), "é"),
                ('-', Some(2), None, "ü😀"),
                ('+', None, Some(2), "ü😀x"),
                (' ', Some(3), Some(3), "z")
            ]
        );

        let host = host();
        let mut plugin = on_mr(&host);
        pick(&mut plugin, &host, SHA1);
        let html = changes_html(&mut plugin, &host);
        assert!(html.contains("id=\"f0-2-\"") && html.contains("id=\"f0--2\"") && html.contains("id=\"f0-3-3\""), "{html}");
        assert!(!html.contains('\r'));
    }
}

mod pipeline_tests {
    use super::*;

    const API: &str = "https://gitlab.com/api/v4/projects/g%2Fp";

    const JOBS_41: &str = r#"[
        {"id": 303, "name": "lint", "stage": "test", "status": "failed", "allow_failure": true, "web_url": "https://gitlab.com/g/p/-/jobs/303"},
        {"id": 302, "name": "unit", "stage": "test", "status": "running", "web_url": "https://gitlab.com/g/p/-/jobs/302"},
        {"id": 301, "name": "compile", "stage": "build", "status": "success", "web_url": "https://gitlab.com/g/p/-/jobs/301"}
    ]"#;

    fn pipelines(req: &HttpRequest) -> Result<HttpResponse, Error> {
        let url = req.url.split('?').next().unwrap_or("");

        match (req.method.as_str(), url.strip_prefix(API).unwrap_or("")) {
            ("GET", "/merge_requests/7/pipelines") => {
                ok(r#"[{"id": 41, "status": "running", "web_url": "https://gitlab.com/g/p/-/pipelines/41"},
                    {"id": 40, "status": "failed", "web_url": "https://gitlab.com/g/p/-/pipelines/40"}]"#)
            }
            ("GET", "/pipelines/41/jobs") => ok(JOBS_41),
            ("GET", "/pipelines/40/jobs") => {
                ok(r#"[{"id": 300, "name": "unit", "stage": "test", "status": "failed", "web_url": "https://gitlab.com/g/p/-/jobs/300"}]"#)
            }
            _ => gitlab(req),
        }
    }

    fn host() -> MockHost {
        let host = setup();
        host.on_fetch(pipelines);
        host
    }

    fn on_mr(host: &MockHost) -> GitLab {
        let mut plugin = selected(host);
        render_in(&mut plugin, Section::Pipelines, &host);
        plugin
    }

    fn pick(plugin: &mut GitLab, host: &MockHost, key: &str) {
        handle_in(plugin, Section::Pipelines, ViewEvent::Select(key.into()), &host);
    }

    fn click(plugin: &mut GitLab, host: &MockHost, id: &str) -> View {
        handle_in(plugin, Section::Pipelines, ViewEvent::Button(id.into()), &host);
        render_in(plugin, Section::Pipelines, &host)
    }

    fn rows(view: &View) -> Vec<(&str, Option<&str>, &str, Option<&str>)> {
        let Body::Tree(items) = &view.body else { panic!("{view:?}") };

        items.iter().map(|i| (i.key.as_str(), i.parent.as_deref(), i.label.as_str(), i.icon.as_deref())).collect()
    }

    #[test]
    fn captured_retry_button_cannot_retarget_another_selected_job() {
        let host=host();let mut plugin=on_mr(&host);pick(&mut plugin,&host,"j:301");
        let view=render_in(&mut plugin,Section::Pipelines,&host);let key=view.toolbar.iter().find(|button|button.label=="Retry Job").unwrap().id.clone();
        pick(&mut plugin,&host,"j:303");let _=plugin.handle(DETAILS,ViewEvent::Button(key),&host.invocation());assert!(writes(&host).is_empty());
    }
    #[test]
    fn captured_browser_button_cannot_open_another_selected_job() {
        let host=host();let mut plugin=on_mr(&host);pick(&mut plugin,&host,"j:301");
        let view=render_in(&mut plugin,Section::Pipelines,&host);let key=view.toolbar.iter().find(|button|button.label=="Open on GitLab").unwrap().id.clone();
        pick(&mut plugin,&host,"j:303");let _=plugin.handle(DETAILS,ViewEvent::Button(key),&host.invocation());assert!(host.opened_urls().is_empty());assert!(writes(&host).is_empty());
    }
    #[test]
    fn stale_first_cancel_control_cannot_replace_the_rendered_confirmation() {
        let host=host();let mut plugin=on_mr(&host);pick(&mut plugin,&host,"j:302");
        let view=render_in(&mut plugin,Section::Pipelines,&host);let key=view.toolbar.iter().find(|button|button.label=="Cancel Pipeline").unwrap().id.clone();
        plugin.handle(DETAILS,ViewEvent::Button(key.clone()),&host.invocation()).unwrap();assert!(writes(&host).is_empty());
        assert!(plugin.handle(DETAILS,ViewEvent::Button(key),&host.invocation()).is_err());assert!(writes(&host).is_empty());
        let view=render_in(&mut plugin,Section::Pipelines,&host);let confirm=view.toolbar.iter().find(|button|button.label=="Confirm Cancel Pipeline #41").unwrap().id.clone();
        plugin.handle(DETAILS,ViewEvent::Button(confirm),&host.invocation()).unwrap();assert_eq!(writes(&host)[0].1,format!("{API}/pipelines/41/cancel"));
    }
    #[test]
    fn shows_pipelines_with_jobs_grouped_by_stage_and_status_icons() {
        let host = host();
        let mut plugin = selected(&host);

        let view = render_in(&mut plugin, Section::Pipelines, &host);

        assert_eq!(
            rows(&view),
            [
                ("p:41", None, "#41", Some("run")),
                ("s:41:build", Some("p:41"), "build", Some("success")),
                ("j:301", Some("s:41:build"), "compile", Some("success")),
                ("s:41:test", Some("p:41"), "test", Some("run")),
                ("j:302", Some("s:41:test"), "unit", Some("run")),
                ("j:303", Some("s:41:test"), "lint", Some("warning")),
                ("p:40", None, "#40", Some("error")),
            ]
        );
        let Body::Tree(items) = &view.body else { unreachable!() };
        assert!(items[0].expanded && items[6].expandable && !items[6].expanded);
        assert_eq!(items[0].detail.as_deref(), Some("Pipeline running"));
        assert_eq!(items[5].detail.as_deref(), Some("Failed (allowed to fail)"));
        assert!(!host.requests().iter().any(|r| r.url.contains("/pipelines/40/jobs")), "a collapsed pipeline loads no jobs");

        handle_in(&mut plugin, Section::Pipelines, ViewEvent::Expand("p:40".into()), &host);
        let view = render_in(&mut plugin, Section::Pipelines, &host);

        assert!(rows(&view).contains(&("j:300", Some("s:40:test"), "unit", Some("error"))), "{view:?}");
        assert!(host.refreshed_views().contains(&LIST.to_owned()));
    }

    #[test]
    fn retry_posts_to_the_selected_job() {
        let host = host();
        let mut plugin = on_mr(&host);
        pick(&mut plugin, &host, "j:301");

        let view = click(&mut plugin, &host, "retry");

        assert_eq!(writes(&host), [("POST".to_owned(), format!("{API}/jobs/301/retry"), String::new())]);
        assert_eq!(view.status.as_deref(), Some("Job retried."));
        let reloads = host.requests().iter().filter(|r| r.url.contains("/merge_requests/7/pipelines")).count();
        assert_eq!(reloads, 2, "the tree reloads after the action");
    }

    #[test]
    fn retry_on_a_pipeline_posts_to_pipeline_retry() {
        let host = host();
        let mut plugin = on_mr(&host);
        pick(&mut plugin, &host, "p:40");

        let view = click(&mut plugin, &host, "retry");

        assert_eq!(writes(&host), [("POST".to_owned(), format!("{API}/pipelines/40/retry"), String::new())]);
        assert_eq!(view.status.as_deref(), Some("Pipeline retried."));
    }

    #[test]
    fn cancel_needs_a_second_click() {
        let host = host();
        let mut plugin = on_mr(&host);
        pick(&mut plugin, &host, "p:41");

        let view = click(&mut plugin, &host, "cancel");
        assert!(writes(&host).is_empty());
        assert_eq!(label(&view, "cancel"), "Confirm Cancel Pipeline #41");

        pick(&mut plugin, &host, "j:302");
        pick(&mut plugin, &host, "p:41");
        let view = click(&mut plugin, &host, "cancel");
        assert!(writes(&host).is_empty(), "a selection change cancels the confirm");

        click(&mut plugin, &host, "refresh");
        pick(&mut plugin, &host, "p:41");
        let view_after_other = click(&mut plugin, &host, "cancel");
        assert!(writes(&host).is_empty(), "another action cancels the confirm");
        assert_eq!(label(&view_after_other, "cancel"), "Confirm Cancel Pipeline #41");
        assert_eq!(label(&view, "cancel"), "Confirm Cancel Pipeline #41");

        let view = click(&mut plugin, &host, "cancel");

        assert_eq!(writes(&host), [("POST".to_owned(), format!("{API}/pipelines/41/cancel"), String::new())]);
        assert_eq!(view.status.as_deref(), Some("Pipeline canceled."));
        assert_eq!(label(&view, "cancel"), "Cancel Pipeline");
    }

    #[test]
    fn tree_keys_with_non_numeric_ids_send_no_request() {
        let host = host();
        let mut plugin = on_mr(&host);
        let before = host.requests().len();

        for key in ["j:1/../2", "p:1?x", "j:-1", "s:1/../2:test"] {
            pick(&mut plugin, &host, key);
            handle_in(&mut plugin, Section::Pipelines, ViewEvent::Expand(key.into()), &host);
            for button in ["retry", "cancel", "cancel", "browser"] {
                handle_in(&mut plugin, Section::Pipelines, ViewEvent::Button(button.into()), &host);
            }
        }

        assert_eq!(host.requests().len(), before);
        assert!(host.opened_urls().is_empty());
        render_in(&mut plugin, Section::Pipelines, &host);
        assert!(host.requests()[before..].iter().all(|r| !r.url.contains("..") && !r.url.contains("?x") && !r.url.contains("/-1")));
    }

    #[test]
    fn open_in_browser_refuses_a_job_url_on_another_host() {
        let host = setup();
        host.on_fetch(|req| match req.url.contains("/pipelines/41/jobs") {
            true => ok(r#"[{"id": 1, "name": "evil", "stage": "test", "status": "failed", "web_url": "https://evil.test/j/1"},
                    {"id": 2, "name": "script", "stage": "test", "status": "failed", "web_url": "javascript:alert(1)"}]"#),
            false => pipelines(req),
        });
        let mut plugin = on_mr(&host);

        for key in ["j:1", "j:2"] {
            pick(&mut plugin, &host, key);
            let view = click(&mut plugin, &host, "browser");
            assert!(view.status.as_deref().unwrap_or("").contains("refusing the URL"), "{view:?}");
        }
        assert!(host.opened_urls().is_empty());

        pick(&mut plugin, &host, "p:41");
        click(&mut plugin, &host, "browser");

        assert_eq!(host.opened_urls(), ["https://gitlab.com/g/p/-/pipelines/41"]);
    }

    #[test]
    fn retry_is_hidden_for_a_running_job() {
        let host = host();
        let mut plugin = on_mr(&host);
        pick(&mut plugin, &host, "j:302");

        let view = click(&mut plugin, &host, "retry");

        assert!(!view.toolbar.iter().any(|b| button_matches(&b.id,"retry")));
        assert!(writes(&host).is_empty());
    }

    #[test]
    fn a_merge_request_without_pipelines_shows_a_message() {
        let host = setup();
        host.on_fetch(|req| match req.url.contains("/merge_requests/7/pipelines") {
            true => ok("[]"),
            false => gitlab(req),
        });
        let mut plugin = selected(&host);

        let view = render_in(&mut plugin, Section::Pipelines, &host);

        assert_eq!(view.body, Body::Message("!7 has no pipelines.".into()));
    }

    #[test]
    fn a_failed_retry_reports_the_gitlab_error_without_the_token() {
        let host = setup();
        host.on_fetch(|req| match req.method == "POST" {
            true => Ok(HttpResponse { status: 403, headers: vec![], body: br#"{"message":"403 Forbidden"}"#.to_vec() }),
            false => pipelines(req),
        });
        let mut plugin = on_mr(&host);
        pick(&mut plugin, &host, "j:301");

        let view = click(&mut plugin, &host, "retry");

        assert_eq!(view.status.as_deref(), Some("GitLab returned 403: 403 Forbidden"));
        assert!(!format!("{view:?}").contains("glpat-x"));
        assert!(host.notifications().iter().all(|n| !format!("{n:?}").contains("glpat-x")));
        assert!(host.logs().iter().all(|(_, line)| !line.contains("glpat-x")));
        assert!(host.requests().iter().all(|r| !r.url.contains("glpat-x")));
    }

    #[test]
    fn a_failed_retry_reloads_the_tree() {
        let host = setup();
        host.on_fetch(|req| match req.method == "POST" {
            true => Ok(HttpResponse { status: 403, headers: vec![], body: br#"{"message":"403 Forbidden"}"#.to_vec() }),
            false => pipelines(req),
        });
        let mut plugin = on_mr(&host);
        pick(&mut plugin, &host, "j:301");

        click(&mut plugin, &host, "retry");

        let reloads = host.requests().iter().filter(|r| r.url.contains("/merge_requests/7/pipelines")).count();
        assert_eq!(reloads, 2);
    }

    #[test]
    fn the_view_is_quiet_when_the_setting_is_off() {
        let host = host();
        let mut plugin = selected(&host);
        host.set_setting("pipelineView", "false");
        let before = host.requests().len();

        let view = render_in(&mut plugin, Section::Pipelines, &host);
        pick(&mut plugin, &host, "j:301");
        handle_in(&mut plugin, Section::Pipelines, ViewEvent::Expand("p:40".into()), &host);
        for button in ["retry", "cancel", "cancel", "browser"] {
            handle_in(&mut plugin, Section::Pipelines, ViewEvent::Button(button.into()), &host);
        }

        assert!(matches!(view.body, Body::Message(_)), "{view:?}");
        assert_eq!(host.requests().len(), before);
        assert!(host.opened_urls().is_empty());
    }
}

#[test]
fn creates_a_snippet_from_the_selected_files_with_a_typed_title() {
    let host = setup();
    host.add_file("/project/src/a.rs", "one");
    host.add_file("/project/b.txt", "two");
    host.select_paths(&["/project/src/a.rs", "/project/b.txt", "/project/dir"]);
    for answer in [Some(&["My files"][..]), Some(&["private"]), Some(&[""])] {
        host.answer_next_prompt(answer);
    }

    GitLab::default().run_command("create-snippet", &host.invocation(), None).unwrap();

    assert_eq!(
        writes(&host)[0].2,
        r#"{"files":[{"content":"one","file_path":"src/a.rs"},{"content":"two","file_path":"b.txt"}],"title":"My files","visibility":"private"}"#
    );
}

#[test]
fn cancelling_the_snippet_title_posts_nothing() {
    let host = setup();
    host.open_editor("/project/a.rs", "rust", "x", &[(0, 0, 0, 0)]);
    host.answer_next_prompt(None);

    GitLab::default().run_command("create-snippet", &host.invocation(), None).unwrap();

    assert!(writes(&host).is_empty());
}

#[test]
fn the_status_item_reveals_the_merge_request_view() {
    let host = branch_on_merge_request_8();
    let mut plugin = GitLab::default();
    plugin.handle_events(&[Event::VcsChanged("/project".into())], &host.invocation());

    plugin.run_command("show-branch-merge-request", &host.invocation(), None).unwrap();

    assert_eq!(host.revealed_views(), [LIST]);
}

#[test]
fn typed_filters_are_added_to_the_quick_filter_and_cleared() {
    let host = setup();
    let mut plugin = GitLab::default();
    plugin.render(LIST, None, &host.invocation()).unwrap();

    host.answer_next_prompt(Some(&["labels"]));
    host.answer_next_prompt(Some(&["bug"]));
    plugin.handle(LIST, ViewEvent::Button("filter".into()), &host.invocation()).unwrap();
    host.answer_next_prompt(Some(&["search"]));
    host.answer_next_prompt(Some(&["parser x"]));
    plugin.handle(LIST, ViewEvent::Button("filter".into()), &host.invocation()).unwrap();
    let view = plugin.render(LIST, None, &host.invocation()).unwrap();

    let list = host.requests().into_iter().filter(|r| r.url.contains("merge_requests?")).last().unwrap();
    assert!(list.url.contains("state=opened&scope=assigned_to_me&labels=bug&search=parser%20x"), "{}", list.url);
    assert_eq!(label(&view, "filter"), "Assigned to me + 2 typed");

    host.answer_next_prompt(Some(&["clear"]));
    plugin.handle(LIST, ViewEvent::Button("filter".into()), &host.invocation()).unwrap();
    let view = plugin.render(LIST, None, &host.invocation()).unwrap();
    assert_eq!(label(&view, "filter"), "Assigned to me");
}

#[test]
fn dismissing_the_filter_pick_keeps_the_filter() {
    let host = setup();
    let mut plugin = GitLab::default();
    plugin.render(LIST, None, &host.invocation()).unwrap();

    plugin.handle(LIST, ViewEvent::Button("filter".into()), &host.invocation()).unwrap();
    let view = plugin.render(LIST, None, &host.invocation()).unwrap();

    assert_eq!(label(&view, "filter"), "Assigned to me");
}

#[test]
fn the_edit_button_replaces_labels_reviewers_and_milestone() {
    let host = setup();
    let mut plugin = selected(&host);
    let ctx = host.invocation();

    host.answer_next_prompt(Some(&["labels"]));
    host.answer_next_prompt(Some(&["bug", "ui"]));
    plugin.handle(DETAILS, ViewEvent::Button("edit".into()), &ctx).unwrap();
    plugin.render(DETAILS, None, &ctx).unwrap();
    host.answer_next_prompt(Some(&["reviewers"]));
    host.answer_next_prompt(Some(&["3"]));
    plugin.handle(DETAILS, ViewEvent::Button("edit".into()), &ctx).unwrap();
    plugin.render(DETAILS, None, &ctx).unwrap();
    host.answer_next_prompt(Some(&["milestone"]));
    host.answer_next_prompt(Some(&["40"]));
    plugin.handle(DETAILS, ViewEvent::Button("edit".into()), &ctx).unwrap();

    let bodies: Vec<String> = writes(&host).into_iter().map(|w| w.2).collect();
    assert_eq!(bodies, [r#"{"labels":"bug,ui"}"#, r#"{"reviewer_ids":[3]}"#, r#"{"milestone_id":40}"#]);
}

#[test]
fn editing_the_title_with_a_command_and_dismissing() {
    let host = setup();
    let mut plugin = selected(&host);

    host.answer_next_prompt(Some(&["title"]));
    host.answer_next_prompt(Some(&["Parser v2"]));
    let message = plugin.run_command("edit-merge-request", &host.invocation(), None).unwrap();
    assert_eq!(message.as_deref(), Some("Title updated."));
    assert_eq!(writes(&host)[0].2, r#"{"title":"Parser v2"}"#);

    host.answer_next_prompt(Some(&["assignees"]));
    host.answer_next_prompt(None);
    let message = plugin.run_command("edit-merge-request", &host.invocation(), None).unwrap();
    assert_eq!(message, None);
    assert_eq!(writes(&host).len(), 1);
}

#[test]
fn project_view_opens_a_file_blob_and_a_folder_tree() {
    let host = setup();
    host.add_file("/project/src/a b.rs", "x");

    host.select_paths(&["/project/src/a b.rs"]);
    GitLab::default().run_command("open-in-browser", &host.invocation(), None).unwrap();
    host.select_paths(&["/project/src"]);
    GitLab::default().run_command("open-in-browser", &host.invocation(), None).unwrap();

    assert_eq!(host.opened_urls(), ["https://gitlab.com/g/p/-/blob/abc123/src/a%20b.rs", "https://gitlab.com/g/p/-/tree/abc123/src"]);
}

#[test]
fn selecting_a_merge_request_enables_its_palette_commands() {
    let host = setup();
    assert!(!host.context_keys().contains_key("mrSelected"));

    let _plugin = selected(&host);

    assert_eq!(host.context_keys().get("mrSelected"), Some(&openidea_plugin::ContextValue::Flag(true)));
}

#[test]
fn project_view_copies_the_selected_file_link_not_the_editor() {
    let host = setup();
    host.add_file("/project/src/b.rs", "x");
    host.open_editor("/project/src/a.rs", "rust", "x", &[(0, 0, 0, 0)]);
    host.select_paths(&["/project/src/b.rs"]);

    GitLab::default().run_command("copy-link", &host.invocation(), None).unwrap();

    assert_eq!(host.clipboard().as_deref(), Some("https://gitlab.com/g/p/-/blob/abc123/src/b.rs"));
}

mod ci_aid_tests {
    use super::*;
    use crate::ci;
    use openidea_plugin::Position;

    fn labels(yaml_with_cursor: &str) -> Vec<String> {
        let at = yaml_with_cursor.find('|').unwrap();
        let yaml = yaml_with_cursor.replace('|', "");
        ci::completions(&yaml, Some(at)).into_iter().map(|c| c.label).collect()
    }

    fn codes(host: &MockHost, yaml: &str) -> Vec<String> {
        host.add_file("/project/.gitlab-ci.yml", yaml);
        let mut plugin = GitLab::default();
        plugin.handle_events(&[Event::FileSaved("/project/.gitlab-ci.yml".into())], &host.invocation());
        let found = host.diagnostics().get("/project/.gitlab-ci.yml").cloned().unwrap_or_default();

        found.into_iter().filter_map(|d| d.code).collect()
    }

    #[test]
    fn completion_after_non_ascii_does_not_panic() {
        labels("test:\n  script: echo café|");

        let seeds = ["test:\n  script: echo café", "build:\n  script: $é${C", "café: 😀 $CI_"];
        for seed in seeds {
            for offset in (0..=seed.len()).filter(|o| seed.is_char_boundary(*o)) {
                ci::completions(seed, Some(offset));
            }
        }
    }

    #[test]
    fn top_level_keys_complete_to_global_keywords() {
        assert_eq!(labels("sta|"), ["stages"]);
        assert!(labels("|").contains(&"include".to_owned()));
    }

    #[test]
    fn job_keys_complete_inside_a_job_but_not_inside_variables() {
        assert_eq!(labels("build:\n  scr|"), ["script"]);
        assert!(labels("variables:\n  scr|").is_empty());
        assert!(labels("build:\n  script:\n    - ech|").is_empty());
    }

    #[test]
    fn values_complete_for_when_stage_and_job_references() {
        assert_eq!(labels("a:\n  when: man|"), ["manual"]);
        assert_eq!(labels("a:\n  rules:\n    - when: ne|"), ["never"]);
        assert_eq!(labels("a:\n  allow_failure: t|"), ["true"]);
        assert_eq!(labels("stages: [lint, ship]\na:\n  stage: sh|"), ["ship"]);
        assert_eq!(labels("a:\n  stage: |").len(), 5);
        assert_eq!(labels(".base:\n  script: x\nb:\n  extends: .ba|"), [".base"]);
        assert_eq!(labels("a:\n  script: x\nb:\n  needs: [a, |"), ["a", "b"]);
        assert_eq!(labels("a:\n  script: x\nb:\n  needs:\n    - a|"), ["a"]);
        assert!(labels("a:\n  script:\n    - a|").is_empty());
        assert!(labels("a:\n  script: echo when: m|").is_empty());
    }

    #[test]
    fn reference_completes_the_job_then_the_key() {
        assert_eq!(labels(".setup:\n  script: x\nb:\n  script:\n    - !reference [.se|"), [".setup"]);
        assert_eq!(labels(".setup:\n  script: x\nb:\n  script:\n    - !reference [.setup, scr|"), ["script"]);
    }

    #[test]
    fn reference_to_a_missing_job_warns_and_resolves_to_the_job() {
        let yaml = ".setup:\n  script: x\nb:\n  script:\n    - !reference [.setup, script]\n    - !reference [.gone, script]\n";
        assert_eq!(codes(&setup(), yaml), ["undefined-reference"]);

        let file = ci::parse(yaml);
        let at = yaml.find("[.setup").unwrap() + 3;
        let target = ci::target_at(&file, at).unwrap();
        assert_eq!(&yaml[target], ".setup");
    }

    #[test]
    fn ci_variables_complete_after_dollar_and_braces() {
        assert_eq!(labels("build:\n  script:\n    - echo $CI_COMMIT_SHORT|"), ["CI_COMMIT_SHORT_SHA"]);
        assert!(labels("a: ${CI_PROJECT_P|").contains(&"CI_PROJECT_PATH".to_owned()));
    }

    #[test]
    fn completion_replaces_the_typed_prefix_and_ignores_other_files() {
        let mut plugin = GitLab::default();
        let doc = Document { path: "/project/.gitlab-ci.yml".into(), text: "b:\n  scr".into(), ..Document::default() };
        let items = plugin.completion(&doc, Position::new(1, 5), None);

        assert_eq!(items[0].insert_text, "script:");
        assert_eq!(items[0].replace, Some(Range::new(Position::new(1, 2), Position::new(1, 5))));
        let other = Document { path: "/project/a.yml".into(), ..doc };
        assert!(plugin.completion(&other, Position::new(1, 5), None).is_empty());
    }

    #[test]
    fn reference_tags_in_scripts_give_no_findings() {
        let host = MockHost::install();
        let yaml = ".setup:\n  script: x\nbuild:\n  script:\n    - !reference [.setup, script]\n    - echo hi\n";

        assert!(codes(&host, yaml).is_empty());
    }

    #[test]
    fn local_include_supplies_jobs_and_stages() {
        let host = MockHost::install();
        host.set_vcs_status(Some(status("main")));
        host.add_file("/project/ci/base.yml", "stages: [lint]\n.base:\n  script: [ls]\n");
        let yaml = "include:\n  - local: /ci/base.yml\nbuild:\n  extends: .base\n  stage: lint\n";

        assert!(codes(&host, yaml).is_empty());
        assert_eq!(codes(&host, "include: ci/base.yml\nbuild:\n  extends: .nope\n  stage: other\n"), ["undefined-job", "undefined-stage"]);
    }

    #[test]
    fn remote_or_unreadable_include_stays_quiet() {
        let host = MockHost::install();
        host.set_vcs_status(Some(status("main")));
        let remote = "include:\n  - local: /ci/base.yml\n  - remote: https://x.test/a.yml\nbuild:\n  extends: .nope\n";
        assert!(codes(&host, remote).is_empty());

        host.add_file("/project/ci/base.yml", "x:\n  script: [ls]\n");
        assert!(codes(&host, remote).is_empty());
        assert!(codes(&host, "include:\n  - local: /missing.yml\nbuild:\n  extends: .nope\n").is_empty());
    }

    #[test]
    fn include_cycles_terminate() {
        let host = MockHost::install();
        host.set_vcs_status(Some(status("main")));
        host.add_file("/project/a.yml", "include: b.yml\n.a:\n  script: [ls]\n");
        host.add_file("/project/b.yml", "include: a.yml\n");

        assert!(codes(&host, "include: a.yml\nbuild:\n  extends: .a\n").is_empty());
    }
}

#[test]
fn section_buttons_list_the_sections_with_the_active_one_marked() {
    let host = setup();
    let mut plugin = selected(&host);

    let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();

    let ids: Vec<&str> = section_buttons(&view).into_iter().map(|(id, _)| id).collect();
    assert_eq!(ids, ["section:overview", "section:description", "section:changes", "section:commits", "section:files", "section:pipelines", "section:timeline", "section:drafts"]);
    assert_eq!(section_buttons(&view)[0].1, "● Overview");
    assert_eq!(section_buttons(&view)[1].1, "Description");

    let view = changes_view(&mut plugin, &host).unwrap();

    assert_eq!(section_buttons(&view)[0].1, "Overview");
    assert_eq!(section_buttons(&view)[2].1, "● Changes (1)");
}

#[test]
fn clicking_a_section_button_switches_the_view_and_stores_it() {
    let host = setup();
    let mut plugin = selected(&host);

    plugin.handle(DETAILS, ViewEvent::Button("section:changes".into()), &host.invocation()).unwrap();

    assert_eq!(storage::get(sections::SECTION_KEY).unwrap().as_deref(), Some("changes"));
    assert!(host.refreshed_views().contains(&LIST.to_owned()));
    let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();
    assert!(matches!(view.body, Body::Html(_)), "{view:?}");
}

#[test]
fn unknown_stored_section_falls_back_to_overview() {
    let host = setup();
    let mut plugin = selected(&host);
    storage::set(sections::SECTION_KEY, "gone").unwrap();

    let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();

    assert!(matches!(view.body, Body::Form(_)), "{view:?}");
    assert_eq!(section_buttons(&view)[0].1, "● Overview");
}

#[test]
fn no_selected_merge_request_shows_the_select_message_in_every_section() {
    let host = setup();
    let mut plugin = GitLab::default();

    for section in Section::ALL {
        storage::set(sections::SECTION_KEY, section.id()).unwrap();
        let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();

        assert!(matches!(&view.body, Body::Message(m) if m.contains("Select a merge request")), "{}: {view:?}", section.id());
        assert_eq!(section_buttons(&view).len(), Section::ALL.len());
        for (id, _) in section_buttons(&view) {
            plugin.handle(DETAILS, ViewEvent::Button(id.into()), &host.invocation()).unwrap();
        }
    }
}

#[test]
fn changes_section_still_routes_view_messages() {
    let host = setup();
    let mut plugin = selected(&host);
    storage::set(sections::SECTION_KEY, "changes").unwrap();
    plugin.render(DETAILS, None, &host.invocation()).unwrap();

    changes_message(&mut plugin, &host, ViewEvent::Message(r#"{"type":"viewed","path":"src/a.rs","viewed":true}"#.into())).unwrap();

    let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();
    let Body::Html(html) = &view.body else { panic!("{view:?}") };
    assert!(html.contains("src/a.rs"));
}

#[test]
fn badges_use_cached_data_and_make_no_api_call() {
    let host = setup();
    let mut plugin = selected(&host);
    render_in(&mut plugin, Section::Overview, &host);
    let before = host.requests().len();

    let view = render_in(&mut plugin, Section::Overview, &host);

    assert_eq!(host.requests().len(), before);
    assert!(view.toolbar.iter().any(|b| b.id == "section:commits"));
}

#[test]
fn files_section_toggles_viewed_and_refreshes_the_window() {
    let host = setup();
    let mut plugin = selected(&host);
    render_in(&mut plugin, Section::Files, &host);
    let before = host.refreshed_views().len();

    handle_in(&mut plugin, Section::Files, ViewEvent::Button("clear".into()), &host);

    assert!(host.refreshed_views()[before..].contains(&LIST.to_owned()));
}

mod existing_worktree_tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    const SHA: &str = "1111111111111111111111111111111111111111";
    const OTHER: &str = "2222222222222222222222222222222222222222";

    fn record(path: &str, sha: &str, state: &str) -> String {
        format!("worktree {path}\0HEAD {sha}\0{state}\0\0")
    }

    fn current_mr() -> String {
        MR.replace("abc123", SHA)
    }

    fn prepared() -> (MockHost, GitLab) {
        let host = setup();
        host.on_fetch(|request| {
            if request.method == "GET" && request.url.ends_with("/merge_requests/7") { ok(&current_mr()) } else { gitlab(request) }
        });
        let plugin = selected(&host);
        (host, plugin)
    }

    fn list_process(host: &MockHost, output: String) {
        host.on_process("git", move |args, _| {
            assert_eq!(args, ["worktree", "list", "--porcelain", "-z"], "existing worktree action must be read-only");
            Ok(ProcessOutput { status: Some(0), stdout: output.clone(), stderr: String::new() })
        });
    }

    #[test]
    fn opens_only_existing_exact_head_in_new_window_and_preserves_review_text() {
        let (host, mut plugin) = prepared();
        list_process(&host, record("/work/review space", SHA, "branch refs/heads/feature"));
        storage::set(COMMENT_KEY, "unfinished review").unwrap();
        storage::set(AS_DRAFT_KEY, "true").unwrap();
        host.answer_next_prompt(Some(&["/work/review space"]));
        plugin.run_command("open-existing-merge-request-worktree", &host.invocation(), None).unwrap();
        assert_eq!(host.opened_projects(), [("/work/review space".into(), true)]);
        assert_eq!(git_runs(&host), ["worktree list --porcelain -z", "worktree list --porcelain -z"]);
        assert!(writes(&host).is_empty());
        assert_eq!(storage::get(COMMENT_KEY).unwrap().as_deref(), Some("unfinished review"));
        assert_eq!(storage::get(AS_DRAFT_KEY).unwrap().as_deref(), Some("true"));
    }

    #[test]
    fn dismissing_worktree_picker_keeps_project_and_review_state() {
        let (host, mut plugin) = prepared();
        list_process(&host, record("/work/review", SHA, "detached"));
        storage::set(COMMENT_KEY, "draft").unwrap();
        plugin.handle(DETAILS, ViewEvent::Button("open-existing-worktree".into()), &host.invocation()).unwrap();
        assert!(host.opened_projects().is_empty());
        assert_eq!(git_runs(&host).len(), 1);
        assert_eq!(storage::get(COMMENT_KEY).unwrap().as_deref(), Some("draft"));
        assert!(plugin.details.is_some());
    }

    #[test]
    fn worktree_list_distinguishes_branch_fork_owner_and_detached_heads() {
        let mut mr: MergeRequest = serde_json::from_str(&current_mr()).unwrap();
        let output = record("/work/right", SHA, "branch refs/heads/feature")
            + &record("/work/wrong-branch", SHA, "branch refs/heads/unrelated")
            + &record("/work/old", OTHER, "branch refs/heads/feature")
            + &record("/work/detached", SHA, "detached")
            + &record("/work/locked", SHA, "detached\0locked maintenance")
            + &record("/work/pruned", SHA, "detached\0prunable missing directory");
        let paths: Vec<_> = branch::matching_worktrees(&output, &mr).unwrap().into_iter().map(|tree| tree.path).collect();
        assert_eq!(paths, ["/work/right", "/work/detached"]);
        mr.source_project_id = 99;
        let output = record("/work/fork", SHA, "branch refs/heads/fork/ann/feature")
            + &record("/work/other-owner", SHA, "branch refs/heads/fork/bob/feature");
        assert_eq!(branch::matching_worktrees(&output, &mr).unwrap()[0].path, "/work/fork");
        assert_eq!(branch::matching_worktrees(&output, &mr).unwrap().len(), 1);
    }

    #[test]
    fn malformed_or_ambiguous_worktree_lists_are_rejected() {
        for output in [
            "worktree /work/a\0HEAD abc123\0detached\0\0".into(),
            record("relative/path", SHA, "detached"),
            record("/work/../other", SHA, "detached"),
            record("/work/a", SHA, "detached\0branch refs/heads/feature"),
            record("/work/a", SHA, "detached\0HEAD 1111111111111111111111111111111111111111"),
            record("/work/a", SHA, "detached") + &record("/work/a", SHA, "detached"),
            record("/work/a", SHA, "detached").trim_end_matches('\0').to_owned(),
            "x".repeat(1024 * 1024 + 1),
        ] {
            let mr: MergeRequest = serde_json::from_str(&current_mr()).unwrap();
            assert!(branch::matching_worktrees(&output, &mr).is_err());
        }
    }

    #[test]
    fn missing_worktrees_and_denied_git_never_create_a_checkout() {
        let (host, mut plugin) = prepared();
        list_process(&host, record("/work/old", OTHER, "branch refs/heads/feature"));
        assert!(plugin.run_command("open-existing-merge-request-worktree", &host.invocation(), None).is_err());
        assert!(host.opened_projects().is_empty());
        let caps: Vec<_> = CAPS.iter().copied().filter(|cap| *cap != "process:git").collect();
        let host = host.with_capabilities(&caps);
        assert!(matches!(plugin.run_command("open-existing-merge-request-worktree", &host.invocation(), None), Err(Error::Denied(_))));
        assert!(host.opened_projects().is_empty());
        assert!(writes(&host).is_empty());
    }

    #[test]
    fn selected_project_root_and_gitlab_owner_are_rechecked() {
        let (host, mut plugin) = prepared();
        host.set_project_root(Some("/other"));
        assert!(plugin.run_command("open-existing-merge-request-worktree", &host.invocation(), None).is_err());
        assert!(host.process_runs().is_empty());
        host.set_project_root(Some("/project"));
        host.set_vcs_remotes(&[("origin", "git@gitlab.com:other/project.git")]);
        assert!(plugin.run_command("open-existing-merge-request-worktree", &host.invocation(), None).is_err());
        assert!(host.process_runs().is_empty());
        assert!(host.opened_projects().is_empty());
    }

    #[test]
    fn different_target_project_is_rejected_before_listing_worktrees() {
        let (host, mut plugin) = prepared();
        host.on_fetch(|request| {
            if request.method == "GET" && request.url.ends_with("/merge_requests/7") {
                ok(&current_mr().replace("\"target_project_id\": 5", "\"target_project_id\": 99"))
            } else {
                gitlab(request)
            }
        });
        assert!(plugin.run_command("open-existing-merge-request-worktree", &host.invocation(), None).is_err());
        assert!(host.process_runs().is_empty());
        assert!(host.opened_projects().is_empty());
    }

    #[test]
    fn changed_worktree_after_prompt_is_not_opened() {
        let (host, mut plugin) = prepared();
        let calls = Rc::new(Cell::new(0));
        host.on_process("git", move |args, _| {
            assert_eq!(args, ["worktree", "list", "--porcelain", "-z"]);
            let count = calls.get();
            calls.set(count + 1);
            Ok(ProcessOutput {
                status: Some(0),
                stdout: record("/work/review", if count == 0 { SHA } else { OTHER }, "detached"),
                stderr: String::new(),
            })
        });
        host.answer_next_prompt(Some(&["/work/review"]));
        assert!(plugin.run_command("open-existing-merge-request-worktree", &host.invocation(), None).is_err());
        assert!(host.opened_projects().is_empty());
    }

    #[test]
    fn changed_merge_request_head_after_prompt_is_not_opened() {
        let (host, mut plugin) = prepared();
        list_process(&host, record("/work/review", SHA, "detached"));
        let calls = Rc::new(Cell::new(0));
        host.on_fetch(move |request| {
            if request.method == "GET" && request.url.ends_with("/merge_requests/7") {
                let count = calls.get();
                calls.set(count + 1);
                ok(&current_mr().replace(SHA, if count == 0 { SHA } else { OTHER }))
            } else {
                gitlab(request)
            }
        });
        host.answer_next_prompt(Some(&["/work/review"]));
        assert!(plugin.run_command("open-existing-merge-request-worktree", &host.invocation(), None).is_err());
        assert!(host.opened_projects().is_empty());
        assert_eq!(git_runs(&host).len(), 1);
    }

    #[test]
    fn contextual_attribute_buttons_edit_existing_choices_and_preserve_review_draft() {
        let (host, mut plugin) = prepared();
        storage::set(COMMENT_KEY, "unfinished review").unwrap();
        host.answer_next_prompt(Some(&["bug", "ui"]));
        plugin.handle(DETAILS, ViewEvent::Button("edit-attribute:labels".into()), &host.invocation()).unwrap();
        assert_eq!(writes(&host).last().unwrap().2, r#"{"labels":"bug,ui"}"#);
        assert_eq!(storage::get(COMMENT_KEY).unwrap().as_deref(), Some("unfinished review"));
        plugin.render(DETAILS, None, &host.invocation()).unwrap();
        let before = writes(&host).len();
        plugin.handle(DETAILS, ViewEvent::Button("edit-attribute:title".into()), &host.invocation()).unwrap();
        assert_eq!(writes(&host).len(), before, "dismissal must not write");
    }

    #[test]
    fn contextual_attribute_rejects_changed_owner_after_choice_without_writing() {
        let (host, mut plugin) = prepared();
        let calls = Rc::new(Cell::new(0));
        host.on_fetch(move |request| {
            if request.method == "GET" && request.url.ends_with("/merge_requests/7") {
                let count = calls.get();
                calls.set(count + 1);
                ok(&current_mr()
                    .replace("\"source_project_id\": 5", if count == 0 { "\"source_project_id\": 5" } else { "\"source_project_id\": 99" }))
            } else {
                gitlab(request)
            }
        });
        host.answer_next_prompt(Some(&["new title"]));
        assert!(plugin.handle(DETAILS, ViewEvent::Button("edit-attribute:title".into()), &host.invocation()).is_err());
        assert!(writes(&host).is_empty());
    }
}

mod content_snapshot_tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    const FRAME: &str = "0123456789abcdef0123456789abcdef";
    const OTHER_FRAME: &str = "fedcba9876543210fedcba9876543210";
    const SHA: &str = "1111111111111111111111111111111111111111";
    const NEXT_SHA: &str = "2222222222222222222222222222222222222222";

    fn prepared() -> (MockHost, GitLab) {
        let host = setup();
        host.on_fetch(|request| {
            if request.method == "GET" && request.url.ends_with("/merge_requests/7") {
                ok(&MR.replace("abc123", SHA))
            } else {
                gitlab(request)
            }
        });
        let plugin = selected(&host);
        (host, plugin)
    }
    fn open_in_frame(host: &MockHost, plugin: &mut GitLab, button: &str, frame: &str) -> Result<(), Error> {
        ui_kit::web::reveal_view(DETAILS, include_str!("ui_config.json"))?;
        host.answer_next_prompt(Some(&[button]));
        let message = serde_json::json!({"channel":"ui-kit","sequence":1,"targetView":DETAILS,"frameId":frame,"project":"/project", "op":"event",
            "event":{"kind":"button","key":"more-actions"}})
        .to_string();
        plugin.handle(LIST, ViewEvent::Message(message), &host.invocation())?;
        let response: serde_json::Value = serde_json::from_str(&host.view_messages().last().unwrap().1).unwrap();
        if let Some(error) = response["error"].as_str() {
            return Err(Error::Failed(error.into()));
        }
        Ok(())
    }
    fn opened(host: &MockHost) -> serde_json::Value {
        host.view_messages()
            .into_iter()
            .filter_map(|(_, text)| serde_json::from_str::<serde_json::Value>(&text).ok())
            .find(|value| value["channel"] == "gitlab-content-open")
            .unwrap()["payload"]
            .clone()
    }
    fn action(host: &MockHost, plugin: &mut GitLab, key: &str, action: &str) -> serde_json::Value {
        let message =
            serde_json::json!({"op":"content-tab-action","requestId":"ct-1","key":key,"action":action,"content":"FORGED EDITOR TEXT"})
                .to_string();
        plugin.handle(LIST, ViewEvent::Message(message), &host.invocation()).unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&host.view_messages().last().unwrap().1).unwrap();
        assert_eq!(envelope["channel"], "gitlab-content-reply");
        assert!(envelope.get("op").is_none(), "broadcast reply must not directly resolve a host pending request");
        assert_eq!(envelope["frameId"], key.split('-').nth(1).unwrap());
        envelope["payload"].clone()
    }

    #[test]
    fn missing_project_content_callback_still_gets_its_filtered_error_reply() {
        let (host, mut plugin) = prepared();
        open_in_frame(&host, &mut plugin, "open-overview-snapshot", FRAME).unwrap();
        let key = plugin.content_snapshots.keys().next().unwrap().clone();
        let before = host.requests().len();
        host.set_project_root(None);
        let reply = action(&host, &mut plugin, &key, "reload");
        assert!(reply["error"].as_str().unwrap().contains("No project"));
        assert_eq!(reply["saved"], false);
        assert_eq!(host.requests().len(), before);
    }

    #[test]
    fn overview_snapshot_is_head_bound_readonly_and_cached_reload_never_fetches() {
        let (host, mut plugin) = prepared();
        storage::set(COMMENT_KEY, "unfinished review").unwrap();
        open_in_frame(&host, &mut plugin, "open-overview-snapshot", FRAME).unwrap();
        let tab = opened(&host);
        assert_eq!(tab["op"], "open-content-tab");
        assert_eq!(tab["readOnly"], true);
        assert!(tab["content"].as_str().unwrap().contains(SHA));
        assert!(tab["content"].as_str().unwrap().contains("Adds it"));
        assert!(tab["key"].as_str().unwrap().len() <= 128);
        let before = host.requests().len();
        let reply = action(&host, &mut plugin, tab["key"].as_str().unwrap(), "reload");
        assert_eq!(host.requests().len(), before, "cached callback must not fetch");
        assert_eq!(reply["content"], tab["content"]);
        assert!(reply.get("saved").is_none(), "successful content replacement must update the readonly baseline");
        assert_eq!(reply["readOnly"], true);
        assert_ne!(reply["content"], "FORGED EDITOR TEXT");
        assert!(host.process_runs().is_empty());
        assert!(writes(&host).is_empty());
        assert_eq!(storage::get(COMMENT_KEY).unwrap().as_deref(), Some("unfinished review"));
    }

    #[test]
    fn diff_snapshot_uses_existing_api_and_return_to_review_preserves_draft() {
        let (host, mut plugin) = prepared();
        storage::set(COMMENT_KEY, "my draft").unwrap();
        open_in_frame(&host, &mut plugin, "open-diff-snapshot", FRAME).unwrap();
        let tab = opened(&host);
        assert_eq!(tab["language"], "diff");
        let content = tab["content"].as_str().unwrap();
        assert!(content.contains("--- src/a.rs\n+++ src/a.rs\n@@ -3,2 +3,2 @@"));
        assert!(content.contains("may omit changes"));
        let before = host.requests().len();
        let reply = action(&host, &mut plugin, tab["key"].as_str().unwrap(), "review");
        assert!(reply.get("error").is_none());
        assert_eq!(host.requests().len(), before);
        assert_eq!(Section::stored(), Section::Changes);
        assert_eq!(host.revealed_views().last().map(String::as_str), Some(LIST));
        assert_eq!(storage::get(COMMENT_KEY).unwrap().as_deref(), Some("my draft"));
    }

    #[test]
    fn snapshot_rejects_wrong_project_selection_unknown_key_and_save() {
        let (host, mut plugin) = prepared();
        open_in_frame(&host, &mut plugin, "open-overview-snapshot", FRAME).unwrap();
        let tab = opened(&host);
        let key = tab["key"].as_str().unwrap();
        let before = host.requests().len();
        let reply = action(&host, &mut plugin, key, "save");
        assert!(reply["error"].as_str().unwrap().contains("read-only"));
        assert_eq!(reply["saved"], false);
        assert!(
            action(&host, &mut plugin, &format!("gl1-{FRAME}-{SHA}-0000000000000000-o"), "reload")["error"]
                .as_str()
                .unwrap()
                .contains("expired")
        );
        storage::set(SELECTED_KEY, "8").unwrap();
        assert!(action(&host, &mut plugin, key, "reload").get("error").is_some());
        storage::set(SELECTED_KEY, "7").unwrap();
        host.set_project_root(Some("/other"));
        assert!(action(&host, &mut plugin, key, "reload").get("error").is_some());
        assert_eq!(host.requests().len(), before);
        assert!(writes(&host).is_empty());
    }

    #[test]
    fn explicit_refresh_updates_same_head_and_failure_keeps_old_snapshot_for_retry() {
        let (host, mut plugin) = prepared();
        open_in_frame(&host, &mut plugin, "open-overview-snapshot", FRAME).unwrap();
        let tab = opened(&host);
        let key = tab["key"].as_str().unwrap();
        host.on_fetch(|request| {
            if request.method == "GET" && request.url.ends_with("/merge_requests/7") {
                ok(&MR.replace("abc123", SHA).replace("Adds it", "Updated description"))
            } else {
                gitlab(request)
            }
        });
        let refreshed = action(&host, &mut plugin, key, "refresh");
        assert!(refreshed["content"].as_str().unwrap().contains("Updated description"));
        assert!(refreshed.get("saved").is_none(), "successful refresh must update baseline");
        host.on_fetch(|_| Err(Error::Failed("private server failure that must not be echoed".into())));
        let failed = action(&host, &mut plugin, key, "refresh");
        assert!(failed.get("error").is_some());
        assert!(!failed.to_string().contains("private server failure"));
        let cached = action(&host, &mut plugin, key, "reload");
        assert!(cached["content"].as_str().unwrap().contains("Updated description"));
        host.on_fetch(|request| {
            if request.method == "GET" && request.url.ends_with("/merge_requests/7") {
                ok(&MR.replace("abc123", SHA))
            } else {
                gitlab(request)
            }
        });
        assert!(action(&host, &mut plugin, key, "refresh").get("error").is_none());
    }

    #[test]
    fn content_refresh_remote_timeout_fits_fifteen_second_host_callback_window() {
        let (h,mut plugin)=prepared();open_in_frame(&h,&mut plugin,"open-overview-snapshot",FRAME).unwrap();let tab=opened(&h);
        h.on_fetch(|r| {assert!(r.timeout_ms.is_some_and(|ms|ms>0&&ms<=10_000),"{:?}",r.timeout_ms);if r.method=="GET"&&r.url.ends_with("/merge_requests/7"){ok(&MR.replace("abc123",SHA))}else{gitlab(r)}});
        assert!(action(&h,&mut plugin,tab["key"].as_str().unwrap(),"refresh").get("error").is_none());
    }
    #[test]
    fn expired_content_refresh_retains_last_good_and_budget_scope_does_not_poison_retry() {
        let (h,mut plugin)=prepared();open_in_frame(&h,&mut plugin,"open-overview-snapshot",FRAME).unwrap();let tab=opened(&h);let key=tab["key"].as_str().unwrap();
        h.on_fetch(|r| {api::expire_callback_read_budget();if r.method=="GET"&&r.url.ends_with("/merge_requests/7"){ok(&MR.replace("abc123",SHA).replace("Adds it","Late unseen data"))}else{gitlab(r)}});
        let failed=action(&h,&mut plugin,key,"refresh");assert!(failed["error"].is_string());assert!(failed.get("content").is_none());let before=h.requests().len();
        assert_eq!(action(&h,&mut plugin,key,"reload")["content"],tab["content"]);assert_eq!(h.requests().len(),before);
        h.on_fetch(|r|if r.method=="GET"&&r.url.ends_with("/merge_requests/7"){ok(&MR.replace("abc123",SHA))}else{gitlab(r)});
        assert!(action(&h,&mut plugin,key,"refresh").get("error").is_none());
    }
    #[test]
    fn snapshot_refresh_cannot_rebind_to_another_head() {
        let (host, mut plugin) = prepared();
        open_in_frame(&host, &mut plugin, "open-diff-snapshot", FRAME).unwrap();
        let tab = opened(&host);
        host.on_fetch(|request| {
            if request.method == "GET" && request.url.ends_with("/merge_requests/7") {
                ok(&MR.replace("abc123", NEXT_SHA))
            } else {
                gitlab(request)
            }
        });
        assert!(action(&host, &mut plugin, tab["key"].as_str().unwrap(), "refresh").get("error").is_some());
        assert_eq!(action(&host, &mut plugin, tab["key"].as_str().unwrap(), "reload")["content"], tab["content"]);
    }

    #[test]
    fn head_race_while_loading_diff_does_not_publish_snapshot() {
        let (host, mut plugin) = prepared();
        let count = Rc::new(Cell::new(0));
        host.on_fetch(move |request| {
            if request.method == "GET" && request.url.ends_with("/merge_requests/7") {
                let current = count.get();
                count.set(current + 1);
                ok(&MR.replace("abc123", if current == 0 { SHA } else { NEXT_SHA }))
            } else {
                gitlab(request)
            }
        });
        assert!(open_in_frame(&host, &mut plugin, "open-diff-snapshot", FRAME).is_err());
        assert!(plugin.content_snapshots.is_empty());
        assert!(!host.view_messages().iter().any(|(_, message)| message.contains("gitlab-content-open")));
    }

    #[test]
    fn oversized_multibyte_snapshot_is_bounded_and_nul_free() {
        let (host, mut plugin) = prepared();
        host.on_fetch(|request| {
            if request.method == "GET" && request.url.ends_with("/merge_requests/7") {
                let mut mr: serde_json::Value = serde_json::from_str(&MR.replace("abc123", SHA)).unwrap();
                mr["description"] = ("\0🦀".repeat(150_000)).into();
                ok(&mr.to_string())
            } else {
                gitlab(request)
            }
        });
        open_in_frame(&host, &mut plugin, "open-overview-snapshot", FRAME).unwrap();
        let tab = opened(&host);
        let content = tab["content"].as_str().unwrap();
        assert!(content.len() <= 384 * 1024);
        assert!(!content.contains('\0'));
        assert!(content.contains("Snapshot truncated"));
        assert!(content.encode_utf16().count() < 1 << 20);
    }

    #[test]
    fn expired_instance_snapshot_does_not_fetch_or_accept_editor_content() {
        let (host, mut plugin) = prepared();
        open_in_frame(&host, &mut plugin, "open-overview-snapshot", FRAME).unwrap();
        let tab = opened(&host);
        plugin.content_snapshots.clear();
        let before = host.requests().len();
        assert!(action(&host, &mut plugin, tab["key"].as_str().unwrap(), "reload")["error"].as_str().unwrap().contains("expired"));
        assert_eq!(host.requests().len(), before);
        assert!(writes(&host).is_empty());
    }

    #[test]
    fn content_snapshot_cache_is_bounded_and_evicted_tabs_expire() {
        let (host, mut plugin) = prepared();
        let mut first_key = None;
        for digit in 1..=5 {
            let head = digit.to_string().repeat(40);
            host.on_fetch(move |request| {
                if request.method == "GET" && request.url.ends_with("/merge_requests/7") {
                    ok(&MR.replace("abc123", &head))
                } else {
                    gitlab(request)
                }
            });
            open_in_frame(&host, &mut plugin, "open-overview-snapshot", FRAME).unwrap();
            if first_key.is_none() {
                first_key = plugin.content_snapshots.keys().next().cloned();
            }
            assert!(plugin.content_snapshots.len() <= 4);
        }
        assert_eq!(plugin.content_snapshots.len(), 4);
        assert!(action(&host, &mut plugin, &first_key.unwrap(), "reload")["error"].as_str().unwrap().contains("expired"));
    }

    #[test]
    fn snapshot_open_requires_frame_and_origin_is_restored_after_dispatch() {
        let (host, mut plugin) = prepared();
        let requests = host.requests().len();
        assert!(plugin.handle(DETAILS, ViewEvent::Button("open-overview-snapshot".into()), &host.invocation()).is_err());
        assert!(host.view_messages().iter().all(|(_, text)| serde_json::from_str::<serde_json::Value>(text).unwrap()["channel"] == "ui-kit-refresh"));
        assert_eq!(host.requests().len(), requests);
        open_in_frame(&host, &mut plugin, "open-overview-snapshot", FRAME).unwrap();
        assert!(plugin.content_origin_frame.is_none());
        let message: serde_json::Value = host
            .view_messages()
            .iter()
            .filter_map(|(_, text)| serde_json::from_str::<serde_json::Value>(text).ok())
            .find(|value| value["channel"] == "gitlab-content-open")
            .unwrap();
        assert_eq!(message["frameId"], FRAME);
        assert!(message.get("op").is_none());
        assert!(plugin.handle(DETAILS, ViewEvent::Button("open-overview-snapshot".into()), &host.invocation()).is_err());
        plugin.content_origin_frame = Some(OTHER_FRAME.into());
        open_in_frame(&host, &mut plugin, "open-overview-snapshot", FRAME).unwrap();
        assert_eq!(plugin.content_origin_frame.as_deref(), Some(OTHER_FRAME));
    }

    #[test]
    fn two_frames_same_request_id_receive_distinct_routed_snapshot_replies() {
        let (host, mut plugin) = prepared();
        open_in_frame(&host, &mut plugin, "open-overview-snapshot", FRAME).unwrap();
        open_in_frame(&host, &mut plugin, "open-overview-snapshot", OTHER_FRAME).unwrap();
        let opens: Vec<serde_json::Value> = host
            .view_messages()
            .iter()
            .filter_map(|(_, text)| serde_json::from_str::<serde_json::Value>(text).ok())
            .filter(|value| value["channel"] == "gitlab-content-open")
            .collect();
        assert_eq!(opens.len(), 2);
        assert_ne!(opens[0]["payload"]["key"], opens[1]["payload"]["key"]);
        for (open, frame) in opens.iter().zip([FRAME, OTHER_FRAME]) {
            assert_eq!(open["frameId"], frame);
            let key = open["payload"]["key"].as_str().unwrap();
            assert!(key.len() <= 128);
            assert!(key.contains(SHA), "head remains explicit in bounded key");
            let reply = action(&host, &mut plugin, key, "reload");
            assert_eq!(reply["requestId"], "ct-1");
            let envelope: serde_json::Value = serde_json::from_str(&host.view_messages().last().unwrap().1).unwrap();
            assert_eq!(envelope["frameId"], frame);
        }
        plugin.content_snapshots.clear();
        let reply = action(&host, &mut plugin, opens[0]["payload"]["key"].as_str().unwrap(), "reload");
        assert!(reply["error"].as_str().unwrap().contains("expired"));
        assert_eq!(reply["saved"], false);
        let before = host.view_messages().len();
        for key in ["unknown", "gl1-invalid-0000000000000000", "gl1-0123456789ABCDEF0123456789ABCDEF-0000000000000000"] {
            plugin
                .handle(
                    DETAILS,
                    ViewEvent::Message(
                        serde_json::json!({"op":"content-tab-action","requestId":"ct-1","key":key,"action":"reload"}).to_string(),
                    ),
                    &host.invocation(),
                )
                .unwrap();
        }
        assert_eq!(host.view_messages().len(), before, "unroutable errors must not be broadcast");
    }
}

mod cross_pane_refresh_tests {
    use super::*;
    fn ready(host: &MockHost) -> GitLab {
        let mut plugin=GitLab::default();
        plugin.ensure_view_owner(&host.invocation()).unwrap();
        plugin.handle(LIST,ViewEvent::Select("mr:7".into()),&host.invocation()).unwrap();
        plugin.render(DETAILS,None,&host.invocation()).unwrap();
        plugin
    }
    fn notifications(host: &MockHost, after: usize) -> Vec<String> {
        host.view_messages()[after..].iter().filter_map(|(view, text)| {
            let value:serde_json::Value=serde_json::from_str(text).ok()?;
            (value["channel"]=="ui-kit-refresh").then(||value["targetView"].as_str().unwrap_or(view).to_owned())
        }).collect()
    }
    #[test]
    fn selection_notifies_the_other_packaged_pane_without_broadcasting_a_model() {
        let host=setup();let mut plugin=ready(&host);let before=host.view_messages().len();
        plugin.handle(LIST,ViewEvent::Select("mr:7".into()),&host.invocation()).unwrap();
        let views=notifications(&host,before);assert!(!views.is_empty());assert!(views.iter().all(|view|view==DETAILS));
    }
    #[test]
    fn review_reload_notifies_list_and_details_only() {
        let host=setup();let mut plugin=ready(&host);let before=host.view_messages().len();
        plugin.reload_merge_request(7);
        assert_eq!(notifications(&host,before),[DETAILS.to_owned(),LIST.to_owned()]);
    }
    #[test]
    fn stable_render_never_emits_invalidation() {
        let host=setup();let mut plugin=ready(&host);let before=host.view_messages().len();
        plugin.render(DETAILS,None,&host.invocation()).unwrap();plugin.render(DETAILS,None,&host.invocation()).unwrap();
        assert_eq!(host.view_messages().len(),before);
    }
    #[test]
    fn foreign_repository_same_iid_cannot_render_cached_details_or_comment() {
        let host=setup();let mut plugin=ready(&host);
        storage::set(COMMENT_KEY,"private unsent source comment").unwrap();
        let requests=host.requests().len();
        host.set_vcs_remotes(&[("origin","git@gitlab.com:g/other.git")]);
        let view=plugin.render(DETAILS,None,&host.invocation()).unwrap();
        assert!(!format!("{:?}",view.body).contains("private unsent source comment"));
        assert!(!format!("{:?}",view.body).contains("Add parser"));
        assert_eq!(host.requests().len(),requests);assert!(plugin.details.is_none());
    }
    #[test]
    fn foreign_selection_preserves_unsent_comment_and_requires_original_scope() {
        let host=setup();let mut plugin=ready(&host);storage::set(COMMENT_KEY,"private unsent source comment").unwrap();
        let original_scope=storage::get(SELECTED_SCOPE_KEY).unwrap();
        host.set_project_root(Some("/other"));let mut other=status("main");other.root="/other".into();host.set_vcs_status(Some(other));
        assert!(plugin.handle(LIST,ViewEvent::Select("mr:7".into()),&host.invocation()).is_err());
        assert_eq!(storage::get(COMMENT_KEY).unwrap().as_deref(),Some("private unsent source comment"));
        assert_eq!(storage::get(SELECTED_SCOPE_KEY).unwrap(),original_scope);
        host.set_project_root(Some("/project"));host.set_vcs_status(Some(status("main")));
        plugin.handle(LIST,ViewEvent::Select("mr:7".into()),&host.invocation()).unwrap();
        let view=plugin.render(DETAILS,None,&host.invocation()).unwrap();
        assert!(format!("{:?}",view.body).contains("private unsent source comment"));
    }
    #[test]
    fn same_iid_in_new_scope_discards_old_mr_cache_before_loading() {
        let host=setup();let mut plugin=ready(&host);
        plugin.details.as_mut().unwrap().mr.title="old private cache".into();
        host.set_project_root(Some("/other"));let mut other=status("main");other.root="/other".into();host.set_vcs_status(Some(other));
        plugin.handle(LIST,ViewEvent::Select("mr:7".into()),&host.invocation()).unwrap();
        assert!(plugin.details.is_none());
        let view=plugin.render(DETAILS,None,&host.invocation()).unwrap();
        assert!(!format!("{:?}",view.body).contains("old private cache"));
    }
    #[test]
    fn foreign_native_field_event_cannot_overwrite_previous_comment() {
        let host=setup();let mut plugin=ready(&host);storage::set(COMMENT_KEY,"source draft").unwrap();
        host.set_project_root(Some("/other"));let mut other=status("main");other.root="/other".into();host.set_vcs_status(Some(other));
        assert!(plugin.handle(DETAILS,ViewEvent::Field("comment".into(),"foreign write".into()),&host.invocation()).is_err());
        assert_eq!(storage::get(COMMENT_KEY).unwrap().as_deref(),Some("source draft"));
    }
    #[test]
    fn review_reload_invalidates_list_cache_before_its_fresh_model() {
        let host=setup();let mut plugin=ready(&host);
        plugin.render(LIST,None,&host.invocation()).unwrap();assert!(plugin.loaded.is_some());
        plugin.reload_merge_request(7);assert!(plugin.loaded.is_none());
    }
    #[test]
    fn missing_project_cannot_fetch_or_render_owned_cache() {
        let host=setup();let mut plugin=ready(&host);let requests=host.requests().len();host.set_project_root(None);
        let view=plugin.render(DETAILS,None,&host.invocation()).unwrap();
        assert!(!format!("{:?}",view.body).contains("Add parser"));assert!(plugin.details.is_none());assert_eq!(host.requests().len(),requests);
    }
}

mod job_trace_snapshot_tests {
    use super::*;
    use std::{cell::{Cell,RefCell},rc::Rc};
    const FRAME:&str="0123456789abcdef0123456789abcdef";
    const SHA:&str="1111111111111111111111111111111111111111";
    struct Fixture {response:Rc<RefCell<(u16,Vec<u8>)>>,member:Rc<Cell<bool>>,retire_after_trace:Rc<Cell<bool>>}
    fn prepared()->(MockHost,GitLab,Fixture) {
        let h=setup();let response=Rc::new(RefCell::new((200,b"compile ok\n".to_vec())));let member=Rc::new(Cell::new(true));let r=response.clone();let m=member.clone();let retire_after_trace=Rc::new(Cell::new(false));let retire=retire_after_trace.clone();
        h.on_fetch(move |req| {
            let path=req.url.split('?').next().unwrap();
            if path.ends_with("/merge_requests/7") {ok(&MR.replace("abc123",SHA))}
            else if path.ends_with("/merge_requests/7/pipelines") {ok(if m.get(){r#"[{"id":41,"status":"running","web_url":"https://gitlab.com/g/p/-/pipelines/41"}]"#}else{"[]"})}
            else if path.ends_with("/pipelines/41/jobs") {ok(r#"[{"id":301,"name":"compile","stage":"build","status":"running","web_url":"https://gitlab.com/g/p/-/jobs/301"}]"#)}
            else if path.ends_with("/jobs/301/trace") {if retire.get(){m.set(false);}let (status,body)=r.borrow().clone();assert_eq!(req.method,"GET");assert!(req.body.is_none());assert!(req.timeout_ms.is_some_and(|ms|ms>0&&ms<=20_000));Ok(HttpResponse{status,headers:vec![],body})}
            else{gitlab(req)}
        });
        let mut p=selected(&h);let mut context=h.context("aritti.gitlab");context.api_version="1.4".into();<GitLab as Plugin>::activate(&mut p,&context).unwrap();render_in(&mut p,Section::Pipelines,&h);handle_in(&mut p,Section::Pipelines,ViewEvent::Select("j:301".into()),&h);
        (h,p,Fixture{response,member,retire_after_trace})
    }
    fn open(h:&MockHost,p:&mut GitLab)->serde_json::Value {
        let view=render_in(p,Section::Pipelines,h);let key=view.toolbar.iter().find(|button|button.label=="Open Job Trace Snapshot").unwrap().id.clone();
        ui_kit::web::reveal_view(DETAILS, include_str!("ui_config.json")).unwrap();
        let message=serde_json::json!({"channel":"ui-kit","sequence":1,"targetView":DETAILS,"frameId":FRAME,"project":"/project","op":"event","event":{"kind":"button","key":key}}).to_string();p.handle(LIST,ViewEvent::Message(message),&h.invocation()).unwrap();serde_json::from_str::<serde_json::Value>(&h.view_messages().last().unwrap().1).unwrap()
    }
    fn payload(h:&MockHost)->serde_json::Value {h.view_messages().iter().filter_map(|(_,m)|serde_json::from_str::<serde_json::Value>(m).ok()).find(|v|v["channel"]=="gitlab-content-open").unwrap()["payload"].clone()}
    fn action(h:&MockHost,p:&mut GitLab,key:&str,kind:&str)->serde_json::Value {let m=serde_json::json!({"op":"content-tab-action","requestId":"trace-1","key":key,"action":kind}).to_string();p.handle(DETAILS,ViewEvent::Message(m),&h.invocation()).unwrap();serde_json::from_str::<serde_json::Value>(&h.view_messages().last().unwrap().1).unwrap()["payload"].clone()}
    #[test]
    fn managed_glab_trace_requires_membership_and_uses_exact_job_repository() {
        let (h,mut p,fixture)=prepared(); let mut caps=CAPS.to_vec(); caps.push("process:glab"); let h=h.with_capabilities(&caps);
        h.on_process("glab", |args,_| {assert_eq!(args,["version"]);Ok(ProcessOutput{status:Some(0),stdout:"glab 1.130.0 (9b430a11f)\n".into(),stderr:String::new()})});
        h.answer_next_prompt(Some(&["ok"])); p.follow_job_log(&h.invocation()).unwrap();
        let tabs=h.log_tabs();assert_eq!(tabs.len(),1);assert_eq!(tabs[0].log.program,"glab");
        assert_eq!(tabs[0].log.args,["ci","trace","301","--repo","https://gitlab.com/g/p"]);
        fixture.member.set(false);let count=h.process_runs().len();assert!(p.follow_job_log(&h.invocation()).is_err());assert_eq!(h.log_tabs().len(),1);assert_eq!(h.process_runs().len(),count);
    }
    #[test]
    fn managed_glab_trace_cancellation_does_not_open_a_log_tab() {
        let (h,mut p,_)=prepared();let mut caps=CAPS.to_vec();caps.push("process:glab");let h=h.with_capabilities(&caps);
        h.on_process("glab", |_,_|Ok(ProcessOutput{status:Some(0),stdout:"glab version 1.84.0\n".into(),stderr:String::new()}));
        h.answer_next_prompt(None);p.follow_job_log(&h.invocation()).unwrap();assert!(h.log_tabs().is_empty());
    }
    #[test]
    fn explicit_trace_snapshot_is_readonly_scoped_and_cached_reload_performs_no_fetch() {
        let (h,mut p,_)=prepared();let reply=open(&h,&mut p);assert!(reply.get("error").is_none(),"{reply}");let tab=payload(&h);assert_eq!(tab["readOnly"],true);assert_eq!(tab["fileName"],"gitlab-job-301.log");assert_eq!(tab["language"],"plaintext");let content=tab["content"].as_str().unwrap();assert!(content.contains("compile ok")&&content.contains("Job: #301")&&content.contains("not a live stream"));
        let key=tab["key"].as_str().unwrap();let requests=h.requests().len();assert_eq!(action(&h,&mut p,key,"reload")["content"],content);assert_eq!(h.requests().len(),requests);assert!(h.process_runs().is_empty());assert!(h.terminals().is_empty());
        action(&h,&mut p,key,"review");assert_eq!(storage::get(sections::SECTION_KEY).unwrap().as_deref(),Some("pipelines"));
    }
    #[test]
    fn failed_refresh_retains_last_good_without_exposing_raw_job_error_body() {
        let (h,mut p,f)=prepared();open(&h,&mut p);let tab=payload(&h);let key=tab["key"].as_str().unwrap();*f.response.borrow_mut()=(404,b"PRIVATE-FIXTURE-ERROR".to_vec());let error=action(&h,&mut p,key,"refresh");assert!(error["error"].is_string());assert_eq!(error["saved"],false);assert!(!error.to_string().contains("PRIVATE-FIXTURE"));assert_eq!(action(&h,&mut p,key,"reload")["content"],tab["content"]);
    }
    #[test]
    fn missing_current_membership_refuses_trace_before_fetch_and_snapshot_publication() {
        let (h,mut p,f)=prepared();f.member.set(false);let reply=open(&h,&mut p);assert!(reply["error"].is_string());assert!(p.content_snapshots.is_empty());assert!(!h.requests().iter().any(|r|r.url.ends_with("/trace")));
        let (h,mut p,f)=prepared();f.retire_after_trace.set(true);assert!(open(&h,&mut p)["error"].is_string());assert!(h.requests().iter().any(|r|r.url.ends_with("/trace")));assert!(p.content_snapshots.is_empty(),"membership changing during fetch cannot publish captured output");
    }
    #[test]
    fn trace_body_acceptance_utf8_and_display_limits_are_distinct() {
        for body in [vec![b'x';(1<<20)+1],vec![255]] {let (h,mut p,f)=prepared();*f.response.borrow_mut()=(200,body);assert!(open(&h,&mut p)["error"].is_string());assert!(p.content_snapshots.is_empty());}
        let (h,mut p,f)=prepared();*f.response.borrow_mut()=(200,format!("\0{}","😀".repeat(110_000)).into_bytes());open(&h,&mut p);let tab=payload(&h);let text=tab["content"].as_str().unwrap();assert!(text.len()<=384<<10);assert!(text.contains("Snapshot truncated"));assert!(!text.contains('\0'));
    }
    #[test]
    fn native_activation_guard_is_default_false_and_downgrade_retires_trace_cache() {
        let (h,mut p,_)=prepared();open(&h,&mut p);let tab=payload(&h);let old=tab["key"].as_str().unwrap();
        for version in ["1.3","","2.0","1.4-preview"] {let mut c=h.context("aritti.gitlab");c.api_version=version.into();<GitLab as Plugin>::activate(&mut p,&c).unwrap();let before=h.requests().len();assert!(p.open_content_snapshot(&h.invocation(),content_tabs::Kind::Job{pipeline:41,job:301}).is_err());assert_eq!(h.requests().len(),before);assert!(p.content_snapshots.is_empty());}
        assert!(action(&h,&mut p,old,"reload")["error"].is_string());let mut cold=GitLab::default();let before=h.requests().len();assert!(cold.open_content_snapshot(&h.invocation(),content_tabs::Kind::Job{pipeline:41,job:301}).is_err());assert_eq!(h.requests().len(),before);
    }
}

mod timeline_snapshot_tests {
    use super::*;
    use std::sync::{Arc, atomic::{AtomicBool,Ordering}};

    fn ready() -> (MockHost, GitLab) {
        let host=setup();
        let plugin=selected(&host);
        host.on_fetch(|req| {
            if req.url.split('?').next()==Some("https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7") {
                return ok(&MR.replace("\"sha\": \"abc123\"",&format!("\"sha\": \"{}\"","1".repeat(40))));
            }
            gitlab(req)
        });
        storage::set(sections::SECTION_KEY,"timeline").unwrap();
        (host,plugin)
    }
    fn refresh(plugin:&mut GitLab,host:&MockHost)->Result<(),Error> {
        plugin.handle(DETAILS,ViewEvent::Button("timeline-refresh".into()),&host.invocation())
    }
    fn model(plugin:&mut GitLab,host:&MockHost)->(serde_json::Value,Option<String>) {
        let view=plugin.render(DETAILS,None,&host.invocation()).unwrap();
        let Body::Other{kind,data}=view.body else {panic!("expected typed timeline")};
        assert_eq!(kind,"resources");
        (serde_json::from_str(&data).unwrap(),view.status)
    }
    fn comment_key(model:&serde_json::Value)->String {
        model["items"].as_array().unwrap().iter().find(|r|r["cells"][4]=="Why?").unwrap()["key"].as_str().unwrap().into()
    }
    #[test]
    fn explicit_snapshot_has_typed_rows_details_and_render_never_fetches() {
        let (host,mut plugin)=ready();
        host.on_fetch(|_|panic!("cold render must not fetch"));
        assert!(matches!(plugin.render(DETAILS,None,&host.invocation()).unwrap().body,Body::Message(_)));
        host.on_fetch(|r|if r.url.split('?').next()==Some("https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7"){ok(&MR.replace("abc123",&"1".repeat(40)))}else{gitlab(r)});
        refresh(&mut plugin,&host).unwrap();
        host.on_fetch(|_|panic!("cached render must not fetch"));
        let (model,status)=model(&mut plugin,&host);
        assert_eq!(model["columns"],serde_json::json!(["Kind","Author","When","Status","Summary"]));
        assert_eq!(model["items"].as_array().unwrap().len(),4);
        assert!(status.unwrap().contains("Bounded Timeline snapshot"));
        let note=model["items"].as_array().unwrap().iter().find(|r|r["cells"][4]=="Why?").unwrap();
        assert_eq!(note["cells"][1],"bob");assert_eq!(note["cells"][3],"Unresolved");
        assert!(note["details"].to_string().contains("bounded plain text"));
    }
    #[test]
    fn selected_comment_opens_validated_gitlab_url_and_reply_preserves_unsent_draft() {
        let (host,mut plugin)=ready();refresh(&mut plugin,&host).unwrap();
        let key=comment_key(&model(&mut plugin,&host).0);
        storage::set(COMMENT_KEY,"unsent draft").unwrap();
        plugin.handle(DETAILS,ViewEvent::Select(key.clone()),&host.invocation()).unwrap();
        assert_eq!(storage::get(THREAD_KEY).unwrap(),None);
        plugin.handle(DETAILS,ViewEvent::Activate(key),&host.invocation()).unwrap();
        assert_eq!(host.opened_urls(),["https://gitlab.com/g/p/-/merge_requests/7#note_11"]);
        plugin.handle(DETAILS,ViewEvent::Button("timeline-reply".into()),&host.invocation()).unwrap();
        assert_eq!(storage::get(THREAD_KEY).unwrap().as_deref(),Some("d1"));
        assert_eq!(storage::get(NOTE_KEY).unwrap().as_deref(),Some("11"));
        assert_eq!(storage::get(COMMENT_KEY).unwrap().as_deref(),Some("unsent draft"));
        assert_eq!(Section::stored(),Section::Overview);
    }
    #[test]
    fn failed_refresh_retains_last_good_and_explicit_retry_replaces_it() {
        let (host,mut plugin)=ready();refresh(&mut plugin,&host).unwrap();
        let before=model(&mut plugin,&host).0;
        host.on_fetch(|_|Err(Error::Denied("private host detail".into())));
        assert!(refresh(&mut plugin,&host).is_err());
        let (after,status)=model(&mut plugin,&host);assert_eq!(before,after);
        let status=status.unwrap();assert!(status.starts_with("STALE"));assert!(!status.contains("private"));
        host.on_fetch(|r|if r.url.split('?').next()==Some("https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7"){ok(&MR.replace("abc123",&"1".repeat(40)))}else{gitlab(r)});
        refresh(&mut plugin,&host).unwrap();assert!(!model(&mut plugin,&host).1.unwrap().contains("STALE"));
    }
    #[test]
    fn stale_unknown_and_changed_head_actions_cannot_retarget() {
        let (host,mut plugin)=ready();refresh(&mut plugin,&host).unwrap();
        let key=comment_key(&model(&mut plugin,&host).0);
        assert!(plugin.handle(DETAILS,ViewEvent::Activate(format!("{key}:suffix")),&host.invocation()).is_err());
        host.on_fetch(|r|if r.url.split('?').next()==Some("https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7"){ok(&MR.replace("abc123",&"2".repeat(40)))}else{gitlab(r)});
        assert!(plugin.handle(DETAILS,ViewEvent::Activate(key.clone()),&host.invocation()).is_err());
        assert!(host.opened_urls().is_empty());
        refresh(&mut plugin,&host).unwrap();
        assert!(plugin.handle(DETAILS,ViewEvent::Select(key),&host.invocation()).is_err());
        host.set_vcs_remotes(&[("origin","git@gitlab.com:other/repository.git")]);
        let view=plugin.render(DETAILS,None,&host.invocation()).unwrap();
        assert!(matches!(view.body,Body::Message(_)));
    }
    #[test]
    fn packaged_bridge_allows_only_current_resource_keys_and_declared_reply() {
        let (host,mut plugin)=ready();refresh(&mut plugin,&host).unwrap();
        let key=comment_key(&model(&mut plugin,&host).0);
        let send=|plugin:&mut GitLab,event:serde_json::Value| {
            ui_kit::web::reveal_view(DETAILS, include_str!("ui_config.json")).unwrap();
            let msg=serde_json::json!({"channel":"ui-kit","sequence":1,"targetView":DETAILS,"frameId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","project":"/project","op":"event","event":event});
            plugin.handle(LIST,ViewEvent::Message(msg.to_string()),&host.invocation()).unwrap();
            serde_json::from_str::<serde_json::Value>(&host.view_messages().last().unwrap().1).unwrap()
        };
        assert!(send(&mut plugin,serde_json::json!({"kind":"button","key":"timeline-reply"}))["error"].is_string());
        assert!(send(&mut plugin,serde_json::json!({"kind":"select","key":"timeline:forged"}))["error"].is_string());
        assert!(send(&mut plugin,serde_json::json!({"kind":"select","key":key}))["error"].is_null());
        assert!(send(&mut plugin,serde_json::json!({"kind":"button","key":"timeline-reply"}))["error"].is_null());
        assert_eq!(Section::stored(),Section::Overview);
    }
    #[test]
    fn activity_opens_mr_page_without_fabricating_a_reply_target() {
        let (host,mut plugin)=ready();refresh(&mut plugin,&host).unwrap();
        let m=model(&mut plugin,&host).0;
        let key=m["items"].as_array().unwrap().iter().find(|r|r["cells"][0]=="Activity").unwrap()["key"].as_str().unwrap().to_owned();
        plugin.handle(DETAILS,ViewEvent::Select(key.clone()),&host.invocation()).unwrap();
        assert!(plugin.handle(DETAILS,ViewEvent::Button("timeline-reply".into()),&host.invocation()).is_err());
        plugin.handle(DETAILS,ViewEvent::Activate(key),&host.invocation()).unwrap();
        assert_eq!(host.opened_urls(),["https://gitlab.com/g/p/-/merge_requests/7"]);
        assert_eq!(storage::get(THREAD_KEY).unwrap(),None);
    }
    #[test]
    fn head_change_during_capture_does_not_replace_last_good() {
        let (host,mut plugin)=ready();refresh(&mut plugin,&host).unwrap();
        let before=model(&mut plugin,&host).0;
        let changed=Arc::new(AtomicBool::new(false));let flag=changed.clone();
        host.on_fetch(move|r|{
            if r.url.contains("/discussions") {flag.store(true,Ordering::SeqCst);}
            if r.url.split('?').next()==Some("https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7") {return ok(&MR.replace("abc123",&if flag.load(Ordering::SeqCst){"2"}else{"1"}.repeat(40)));}
            gitlab(r)
        });
        assert!(refresh(&mut plugin,&host).is_err());
        assert_eq!(model(&mut plugin,&host).0,before);
    }
}

mod diff_row_path_receipts {
    use super::*;
    #[test]
    fn side_by_side_reads_both_versions_from_gitlab_and_refuses_unlisted_paths() {
        let host=setup();let mut plugin=selected(&host);changes_view(&mut plugin,&host).unwrap();
        host.on_fetch(|r|if r.url.contains("/repository/files/") {ok("file text")} else {gitlab(r)});
        changes_message(&mut plugin,&host,ViewEvent::Message(r#"{"type":"diff","path":"src/a.rs"}"#.into())).unwrap();
        let reads:Vec<String>=host.requests().into_iter().map(|r|r.url).filter(|u|u.contains("/repository/files/")).collect();
        assert_eq!(reads,[
            "https://gitlab.com/api/v4/projects/g%2Fp/repository/files/src%2Fa.rs/raw?ref=abcdef0",
            "https://gitlab.com/api/v4/projects/g%2Fp/repository/files/src%2Fa.rs/raw?ref=abcdef1",
        ]);
        assert!(writes(&host).is_empty());
        let diffs=host.opened_diffs();
        assert_eq!((diffs[0].0.left.as_deref(),diffs[0].0.right.as_deref(),diffs[0].1.as_deref()),(Some("file text"),Some("file text"),Some("src/a.rs")));
        let before=host.requests().len();
        assert!(changes_message(&mut plugin,&host,ViewEvent::Message(r#"{"type":"diff","path":"src/other.rs"}"#.into())).is_err());
        assert_eq!(host.requests().len(),before);
    }
    #[test]
    fn added_file_diff_has_no_left_side_and_skips_the_base_read() {
        let host=setup();
        host.on_fetch(|r|if r.url.contains("/diffs") {ok(r#"[{"old_path":"src/a.rs","new_path":"src/a.rs","new_file":true,"diff":"@@ -0,0 +1 @@\n+c\n"}]"#)}
            else if r.url.contains("/repository/files/") {ok("new text")} else {gitlab(r)});
        let mut plugin=selected(&host);changes_view(&mut plugin,&host).unwrap();
        changes_message(&mut plugin,&host,ViewEvent::Message(r#"{"type":"diff","path":"src/a.rs"}"#.into())).unwrap();
        let reads:Vec<String>=host.requests().into_iter().map(|r|r.url).filter(|u|u.contains("/repository/files/")).collect();
        assert_eq!(reads,["https://gitlab.com/api/v4/projects/g%2Fp/repository/files/src%2Fa.rs/raw?ref=abcdef1"]);
        let diff=&host.opened_diffs()[0].0;
        assert_eq!((diff.left.as_deref(),diff.right.as_deref()),(None,Some("new text")));
    }
    #[test]
    fn registered_current_diff_row_opens_without_remote_mutation() {
        let host=setup();let mut plugin=selected(&host);changes_view(&mut plugin,&host).unwrap();
        let requests=host.requests().len();
        changes_message(&mut plugin,&host,ViewEvent::Message(r#"{"type":"open","path":"src/a.rs","line":4}"#.into())).unwrap();
        assert_eq!(host.opened_locations(),[("/project/src/a.rs".into(),3,0)]);
        assert_eq!(host.requests().len(),requests);assert!(writes(&host).is_empty());
    }

    #[test]
    fn registered_diff_open_refuses_unlisted_traversing_or_nested_owner_rows() {
        for path in ["../outside.rs","src/unlisted.rs","src/a.rs"] {
            let host=setup();let mut plugin=selected(&host);changes_view(&mut plugin,&host).unwrap();
            if path=="src/a.rs" {host.add_project("/project/src");}
            let message=serde_json::json!({"type":"open","path":path,"line":4}).to_string();
            let _=changes_message(&mut plugin,&host,ViewEvent::Message(message));
            assert!(host.opened_locations().is_empty(),"{path}");assert!(writes(&host).is_empty());
        }
    }
    #[test]
    fn registered_diff_viewed_refuses_unlisted_source_row() {
        let host=setup();let mut plugin=selected(&host);changes_view(&mut plugin,&host).unwrap();
        let before=host.storage();let message=r#"{"type":"viewed","path":"not-in-diff.rs","viewed":true}"#;
        let _=changes_message(&mut plugin,&host,ViewEvent::Message(message.into()));
        assert_eq!(host.storage(),before);assert!(writes(&host).is_empty());
    }
}

mod review_callback_context_receipts {
    use super::*;
    #[test]
    fn emitted_current_page_context_accepts_viewed_but_missing_context_refuses() {
        let host=setup();let mut plugin=selected(&host);let view=changes_view(&mut plugin,&host).unwrap();let context=page_context(&view);
        let before=host.storage();let raw=r#"{"type":"viewed","path":"src/a.rs","viewed":true}"#;
        assert!(plugin.handle(DETAILS,ViewEvent::Message(raw.into()),&host.invocation()).is_err());assert_eq!(host.storage(),before);
        let message=serde_json::json!({"type":"viewed","path":"src/a.rs","viewed":true,"reviewContext":context}).to_string();
        plugin.handle(DETAILS,ViewEvent::Message(message),&host.invocation()).unwrap();
        assert_eq!(host.storage().get("viewed:g/p:7").map(String::as_str),Some(r#"{"src/a.rs":"abcdef1"}"#));assert!(writes(&host).is_empty());
    }
    #[test]
    fn old_page_context_cannot_follow_same_project_into_another_local_root() {
        let host=setup();let mut plugin=selected(&host);let context=page_context(&changes_view(&mut plugin,&host).unwrap());
        host.set_project_root(Some("/other"));let mut other=status("main");other.root="/other".into();host.set_vcs_status(Some(other));
        plugin.handle(LIST,ViewEvent::Select("mr:7".into()),&host.invocation()).unwrap();changes_view(&mut plugin,&host).unwrap();let before=host.storage();
        let message=serde_json::json!({"type":"viewed","path":"src/a.rs","viewed":true,"reviewContext":context}).to_string();
        assert!(plugin.handle(DETAILS,ViewEvent::Message(message),&host.invocation()).is_err());assert_eq!(host.storage(),before);assert!(writes(&host).is_empty());
    }

    fn page_context(view:&View)->String {
        let Body::Html(html)=&view.body else {panic!("diff page")};
        html.split("data-review-context=\"").nth(1).and_then(|s|s.split('"').next()).map(|s|s.replace("&quot;","\"").replace("&amp;","&")).unwrap_or_else(||"pre-fix-unbound-page".into())
    }
    #[test]
    fn old_diff_page_cannot_mark_the_same_path_in_another_merge_request() {
        let host=setup();let mut plugin=selected(&host);let context=page_context(&changes_view(&mut plugin,&host).unwrap());
        host.on_fetch(|request| {let mut request=request.clone();let other=request.url.contains("/merge_requests/8");request.url=request.url.replace("/merge_requests/8","/merge_requests/7");let mut response=gitlab(&request)?;
            if other && request.url.split('?').next()==Some("https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7") {response.body=MR.replace("\"iid\": 7","\"iid\": 8").into_bytes();}Ok(response)});
        plugin.handle(LIST,ViewEvent::Select("mr:8".into()),&host.invocation()).unwrap();changes_view(&mut plugin,&host).unwrap();
        let before=host.storage();let requests=host.requests().len();
        let message=serde_json::json!({"type":"viewed","path":"src/a.rs","viewed":true,"reviewContext":context}).to_string();
        let _=plugin.handle(DETAILS,ViewEvent::Message(message),&host.invocation());
        assert_eq!(host.storage(),before);assert_eq!(host.requests().len(),requests);assert!(writes(&host).is_empty());
    }
    #[test]
    fn old_diff_page_cannot_apply_viewed_mark_after_head_refresh() {
        let host=setup();let mut plugin=selected(&host);let context=page_context(&changes_view(&mut plugin,&host).unwrap());
        host.on_fetch(|request| {if request.method=="GET" && request.url.split('?').next()==Some("https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7") {ok(&MR.replace("abcdef1","abcdef2").replace("abc123","def456"))}else{gitlab(request)}});
        plugin.reload_merge_request(7);changes_view(&mut plugin,&host).unwrap();let before=host.storage();
        let message=serde_json::json!({"type":"viewed","path":"src/a.rs","viewed":true,"reviewContext":context}).to_string();
        let _=plugin.handle(DETAILS,ViewEvent::Message(message),&host.invocation());assert_eq!(host.storage(),before);assert!(writes(&host).is_empty());
    }
}

mod review_row_membership_receipts {
    use super::*;
    #[test]
    fn registered_displayed_diff_discussion_can_reply_and_resolve() {
        let host=setup();let mut plugin=selected(&host);changes_view(&mut plugin,&host).unwrap();
        for kind in ["reply","resolve"] {
            let message=serde_json::json!({"type":kind,"discussion":"d1","body":"fixture reply","resolved":true}).to_string();
            changes_message(&mut plugin,&host,ViewEvent::Message(message)).unwrap();
        }
        let posted=writes(&host);assert_eq!(posted.len(),2);assert!(posted[0].1.ends_with("/discussions/d1/notes"));assert!(posted[1].1.ends_with("/discussions/d1?resolved=true"));
    }

    #[test]
    fn registered_changes_render_ignores_empty_discussion_rows_without_panicking() {
        let host=setup();host.on_fetch(|request|if request.method=="GET" && request.url.contains("/discussions") {ok(r#"[{"id":"empty","notes":[]}]"#)} else {gitlab(request)});
        let rendered=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{let mut plugin=selected(&host);changes_view(&mut plugin,&host).unwrap()}));
        assert!(rendered.is_ok());assert!(writes(&host).is_empty());
    }

    #[test]
    fn registered_current_page_cannot_post_unlisted_diff_path_or_line() {
        for (path,old,new) in [("not-in-diff.rs",Some(4u64),None),("src/a.rs",Some(999),None),("src/a.rs",Some(3),Some(4)),("src/a.rs",Some(u64::from(u32::MAX)+5),None)] {
            let host=setup();let mut plugin=selected(&host);changes_view(&mut plugin,&host).unwrap();
            let message=serde_json::json!({"type":"comment","oldPath":path,"newPath":path,"old":old,"new":new,"body":"fixture review text"}).to_string();
            let _=changes_message(&mut plugin,&host,ViewEvent::Message(message));assert!(writes(&host).is_empty(),"{path} {old:?} {new:?}");
        }
    }
    #[test]
    fn registered_current_page_cannot_reply_or_resolve_an_unlisted_discussion() {
        for kind in ["reply","resolve"] {
            let host=setup();let mut plugin=selected(&host);changes_view(&mut plugin,&host).unwrap();
            let message=serde_json::json!({"type":kind,"discussion":"deadbeef","body":"fixture reply","resolved":true}).to_string();
            let _=changes_message(&mut plugin,&host,ViewEvent::Message(message));assert!(writes(&host).is_empty(),"{kind}");
        }
    }
}

mod diff_line_bounds_receipts {
    use super::*;
    fn serve(host:&MockHost,diff:&str) {
        let body=serde_json::json!([{"old_path":"src/a.rs","new_path":"src/a.rs","diff":diff}]).to_string();
        host.on_fetch(move |request|if request.method=="GET" && request.url.contains("/diffs") {ok(&body)}else{gitlab(request)});
    }
    #[test]
    fn registered_changes_render_does_not_overflow_maximum_diff_line_numbers() {
        let host=setup();serve(&host,"@@ -4294967295,2 +4294967295,2 @@\n first\n second\n");
        let view=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{let mut plugin=selected(&host);changes_view(&mut plugin,&host).unwrap()}));
        assert!(view.is_ok());let view=view.unwrap();let Body::Html(html)=view.body else {panic!("diff page")};
        assert!(html.contains("4294967295"));assert!(html.contains("second"));assert!(!html.contains("data-old=\"0\""));assert!(writes(&host).is_empty());
    }
    #[test]
    fn malformed_or_out_of_range_hunk_cannot_guess_line_one_for_a_comment() {
        for diff in ["@@ invalid @@\n x\n","@@ -4294967296 +4294967296 @@\n x\n","@@ -0,0 +0,0 @@\n x\n","@@ -+1 ++1 @@\n x\n"] {
            let host=setup();serve(&host,diff);let mut plugin=selected(&host);changes_view(&mut plugin,&host).unwrap();
            let message=serde_json::json!({"type":"comment","oldPath":"src/a.rs","newPath":"src/a.rs","old":1,"new":1,"body":"fixture comment"}).to_string();
            let _=changes_message(&mut plugin,&host,ViewEvent::Message(message));assert!(writes(&host).is_empty(),"{diff}");
        }
    }
}

mod files_callback_context_receipts {
    use super::*;
    fn switch(host:&MockHost,plugin:&mut GitLab) {
        host.on_fetch(|request| {let mut request=request.clone();let other=request.url.contains("/merge_requests/8");request.url=request.url.replace("/merge_requests/8","/merge_requests/7");let mut response=gitlab(&request)?;
            if other && request.url.split('?').next()==Some("https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7") {response.body=MR.replace("\"iid\": 7","\"iid\": 8").into_bytes();}Ok(response)});
        plugin.handle(LIST,ViewEvent::Select("mr:8".into()),&host.invocation()).unwrap();render_in(plugin,Section::Files,host);
    }
    #[test]
    fn captured_file_row_does_not_mark_the_same_path_in_a_new_merge_request() {
        let host=setup();let mut plugin=selected(&host);let view=render_in(&mut plugin,Section::Files,&host);
        let Body::Tree(items)=view.body else {panic!("file rows")};let key=items.iter().find(|item|item.key.starts_with("file:")).unwrap().key.clone();
        switch(&host,&mut plugin);let before=host.storage();let _=plugin.handle(DETAILS,ViewEvent::Activate(key),&host.invocation());assert_eq!(host.storage(),before);assert!(writes(&host).is_empty());
    }
    #[test]
    fn captured_clear_viewed_button_does_not_clear_a_new_merge_request() {
        let host=setup();let mut plugin=selected(&host);let view=render_in(&mut plugin,Section::Files,&host);
        let key=view.toolbar.iter().find(|button|button.label=="Clear Viewed").unwrap().id.clone();
        switch(&host,&mut plugin);storage::set("viewed:g/p:8",r#"{"src/a.rs":"abcdef1"}"#).unwrap();let before=host.storage();
        let _=plugin.handle(DETAILS,ViewEvent::Button(key),&host.invocation());assert_eq!(host.storage(),before);assert!(writes(&host).is_empty());
    }
}

mod grouped_ui_parity_tests {
    use super::*;
    use serde_json::{Value, json};

    fn send(plugin:&mut GitLab,host:&MockHost,active:&str,operation:Value)->Value {
        let mut request=operation;
        request["channel"]=json!("ui-kit");request["sequence"]=json!(1);
        request["frameId"]=json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        request["project"]=Value::Null;request["targetView"]=json!(active);
        plugin.handle(LIST,ViewEvent::Message(request.to_string()),&host.invocation()).unwrap();
        serde_json::from_str(&host.view_messages().last().unwrap().1).unwrap()
    }
    #[test]
    fn initial_and_details_token_setup_do_not_need_project_ownership() {
        let host=MockHost::install().with_capabilities(&["storage"]);host.set_project_root(None);
        let mut plugin=GitLab::default();
        let answer=send(&mut plugin,&host,LIST,json!({"op":"event","event":{"kind":"button","key":"new-token"}}));
        assert!(answer.get("error").is_none(),"{answer}");
        assert_eq!(host.opened_urls(),[api::new_token_url()]);
        let answer=send(&mut plugin,&host,LIST,json!({"op":"select-view","target":DETAILS}));
        assert_eq!(answer["model"]["activeView"],DETAILS);
        let answer=send(&mut plugin,&host,DETAILS,json!({"op":"event","event":{"kind":"button","key":"new-token"}}));
        assert!(answer.get("error").is_none(),"{answer}");
        assert_eq!(host.opened_urls(),[api::new_token_url(),api::new_token_url()]);
        // The early setup branch must not make any review operation project-free.
        assert!(plugin.handle(LIST,ViewEvent::Button("refresh".into()),&host.invocation()).is_err());
        assert!(plugin.handle(DETAILS,ViewEvent::Button("approve".into()),&host.invocation()).is_err());
        assert!(host.requests().is_empty());
    }

    #[test]
    fn custom_tabs_and_token_setup_work_without_a_project_and_old_tab_actions_refuse() {
        let host=MockHost::install().with_capabilities(&["storage"]);host.set_project_root(None);
        let mut plugin=GitLab::default();
        let answer=send(&mut plugin,&host,LIST,json!({"op":"render"}));
        assert_eq!(answer["model"]["viewTabs"].as_array().unwrap().len(),3);
        let answer=send(&mut plugin,&host,LIST,json!({"op":"select-view","target":PROJECTS}));
        assert_eq!(answer["model"]["activeView"],PROJECTS);
        let answer=send(&mut plugin,&host,PROJECTS,json!({"op":"event","event":{"kind":"button","key":"new-token"}}));
        assert!(answer.get("error").is_none(),"{answer}");
        assert_eq!(host.opened_urls(),[api::new_token_url()]);
        let stale=send(&mut plugin,&host,LIST,json!({"op":"event","event":{"kind":"button","key":"new-token"}}));
        assert!(stale["error"].is_string());assert_eq!(host.opened_urls().len(),1);
        assert!(host.view_messages().iter().all(|(view,_)|view==LIST));
    }
}

#[test]
fn rerequest_review_uses_assigned_reviewer_global_id_and_requires_graphql_acknowledgement() {
    let h=setup();
    h.on_fetch(|request| {
        if request.url.split('?').next().unwrap().ends_with("/merge_requests/7") {return ok(&MR.replace("\"reviewers\": []",r#""reviewers": [{"id":42,"username":"reviewer","name":"Reviewer"}]"#));}
        if request.url.ends_with("/api/graphql") {return ok(r#"{"data":{"mergeRequestReviewerRereview":{"errors":[]}}}"#);}
        gitlab(request)
    });
    let mut plugin=selected(&h);h.answer_next_prompt(Some(&["42"]));h.answer_next_prompt(Some(&["ok"]));
    plugin.run_command("rerequest-review",&h.invocation(),None).unwrap();
    let posted:Vec<_>=h.requests().into_iter().filter(|request|request.url.ends_with("/api/graphql")).collect();assert_eq!(posted.len(),1);
    let body:serde_json::Value=serde_json::from_slice(posted[0].body.as_ref().unwrap()).unwrap();
    assert_eq!(body["variables"],serde_json::json!({"projectPath":"g/p","iid":"7","userId":"gid://gitlab/User/42"}));
    h.on_fetch(|_|ok("{}"));assert!(api::Project("g/p").rerequest_review(7,42).is_err());
}
#[test]
fn rerequest_review_cancelled_picker_sends_no_mutation() {
    let h=setup();h.on_fetch(|request| {
        if request.url.split('?').next().unwrap().ends_with("/merge_requests/7") {return ok(&MR.replace("\"reviewers\": []",r#""reviewers": [{"id":42,"username":"reviewer","name":"Reviewer"}]"#));}gitlab(request)
    });
    let mut plugin=selected(&h);h.answer_next_prompt(None);plugin.run_command("rerequest-review",&h.invocation(),None).unwrap();
    assert!(h.requests().iter().all(|request|request.method=="GET"));
}

#[test]
fn account_slots_use_declared_secret_placeholders_and_expire_previous_identity() {
    let h=setup();let mut plugin=selected(&h);plugin.ensure_account().unwrap();let previous=selection_scope(&gitlab_project(&h.invocation()).unwrap());
    plugin.me=Some(User{id:9,username:"primary".into(),name:"Primary".into()});
    h.set_secret("secondary-token","second-private-token");h.set_setting("accountSlot",r#""secondary""#);plugin.ensure_account().unwrap();
    assert!(plugin.me.is_none()&&plugin.projects.is_none()&&plugin.content_snapshots.is_empty());
    assert_ne!(selection_scope(&gitlab_project(&h.invocation()).unwrap()),previous);
    assert!(plugin.checked_selection(&gitlab_project(&h.invocation()).unwrap(),&h.invocation()).is_err());
    assert_eq!(api::authorization("https://gitlab.com").unwrap(),"Bearer {{secret:secondary-token}}");
    assert!(!api::authorization("https://gitlab.com").unwrap().contains("second-private-token"));
}
#[test]
fn account_slot_change_cannot_reuse_an_unsent_primary_comment() {
    let h=setup();let mut plugin=selected(&h);plugin.ensure_account().unwrap();storage::set(COMMENT_KEY,"Unsent primary comment").unwrap();
    h.set_setting("accountSlot",r#""secondary""#);let before=h.requests().len();
    assert!(plugin.run_command("rerequest-review",&h.invocation(),None).is_err());assert_eq!(h.requests().len(),before);
    assert_eq!(storage::get(COMMENT_KEY).unwrap().as_deref(),Some("Unsent primary comment"));
    h.set_setting("accountSlot",r#""primary""#);plugin.ensure_account().unwrap();
}

#[test]
fn description_markdown_is_sanitized_and_only_snapshot_links_open(){
    let h=setup();let mut mr:serde_json::Value=serde_json::from_str(MR).unwrap();mr["description"]=serde_json::json!("# Description\n\n**Readable** ![pixel](https://tracking.invalid/pixel) [Docs](https://docs.example.com/guide)\n\n<script>untrusted()</script>");let mr=mr.to_string();
    h.on_fetch(move|request|if request.url.split('?').next().unwrap().ends_with("/merge_requests/7"){ok(&mr)}else{gitlab(request)});
    let mut plugin=selected(&h);let view=render_in(&mut plugin,Section::Description,&h);let Body::Html(html)=view.body else{panic!("Markdown description")};
    assert!(html.contains("<strong>Readable</strong>")&&html.contains("[Image: pixel]")&&html.contains("&lt;script&gt;"));assert!(!html.contains("<img")&&!html.contains("<script>untrusted()"));
    plugin.open_markdown_link(&h.invocation(),"https://docs.example.com/guide").unwrap();assert_eq!(h.opened_urls(),["https://docs.example.com/guide"]);assert!(plugin.open_markdown_link(&h.invocation(),"https://forged.invalid/").is_err());assert_eq!(h.opened_urls().len(),1);
    assert!(h.requests().iter().all(|request|!request.url.contains("tracking.invalid")));
}

mod source_cli_checkout {
    use super::*;
    const SHA:&str="0123456789abcdef0123456789abcdef01234567";
    fn prepared()->(MockHost,GitLab){
        let h=setup();let mut caps=CAPS.to_vec();caps.push("process:glab");let h=h.with_capabilities(&caps);
        h.on_fetch(|r|if r.url.split('?').next()==Some("https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7") {ok(&MR.replace("\"source_project_id\": 5","\"source_project_id\": 9").replace("\"abc123\"",&format!("\"{SHA}\"")))}else{gitlab(r)});
        h.on_process("glab",|args,_|{assert_eq!(args,["version"]);Ok(ProcessOutput{status:Some(0),stdout:"glab 1.130.0 (9b430a11f)\n".into(),stderr:String::new()})});
        let p=selected(&h);(h,p)
    }
    #[test]
    fn fork_checkout_launches_exact_reviewed_cli_without_force(){
        let(h,mut p)=prepared();h.answer_next_prompt(Some(&["ok"]));
        p.run_command("checkout-source-cli",&h.invocation(),None).unwrap();
        let terminals=h.terminals();assert_eq!(terminals.len(),1);assert_eq!(terminals[0].program,"glab");
        assert_eq!(terminals[0].args,["mr","checkout","7","--repo","https://gitlab.com/g/p","--branch","review/mr-7-0123456789ab"]);
        assert!(writes(&h).is_empty());assert!(!git_runs(&h).iter().any(|args|args.starts_with("fetch ")||args.starts_with("checkout ")));
    }
    #[test]
    fn fork_checkout_denied_names_permission(){
        let h=setup();let h=h.with_capabilities(CAPS);
        h.on_fetch(|r|if r.url.split('?').next()==Some("https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7") {ok(&MR.replace("\"source_project_id\": 5","\"source_project_id\": 9").replace("\"abc123\"",&format!("\"{SHA}\"")))}else{gitlab(r)});
        let mut p=selected(&h);h.answer_next_prompt(Some(&["ok"]));
        let error=p.checkout_source_cli(&h.invocation()).unwrap_err().to_string();
        assert!(error.contains("needs the process:glab permission"),"{error}");assert!(error.contains("Settings › Plugins › GitLab"),"{error}");
    }
    #[test]
    fn canceled_review_never_probes_or_launches_cli(){let(h,mut p)=prepared();h.answer_next_prompt(None);p.checkout_source_cli(&h.invocation()).unwrap();assert!(h.process_runs().is_empty());assert!(h.terminals().is_empty());}
    #[test]
    fn dirty_worktree_refuses_even_version_probe(){let(h,mut p)=prepared();let mut s=status("main");s.changed_files=1;h.set_vcs_status(Some(s));assert!(p.checkout_source_cli(&h.invocation()).is_err());assert!(h.process_runs().is_empty());assert!(h.terminals().is_empty());}
    #[test]
    fn unsupported_cli_profile_never_launches(){let(h,mut p)=prepared();h.on_process("glab",|_,_|Ok(ProcessOutput{status:Some(0),stdout:"glab 2.0.0\n".into(),stderr:String::new()}));h.answer_next_prompt(Some(&["ok"]));let error=p.checkout_source_cli(&h.invocation()).unwrap_err().to_string();assert_eq!(error,"Checking out with glab needs GitLab CLI glab 1.x (found 2.0.0). Install it, sign in with glab auth login, then retry.");assert!(h.terminals().is_empty());}
    #[test]
    fn any_glab_1x_launches_checkout(){let(h,mut p)=prepared();h.on_process("glab",|_,_|Ok(ProcessOutput{status:Some(0),stdout:"glab version 1.46.1 (2024-10-01)\n".into(),stderr:String::new()}));h.answer_next_prompt(Some(&["ok"]));p.checkout_source_cli(&h.invocation()).unwrap();assert_eq!(h.terminals().len(),1);}
    #[test]
    fn moving_source_identity_refuses_launch(){
        let(h,mut p)=prepared();let moved=std::rc::Rc::new(std::cell::Cell::new(false));let m=moved.clone();
        h.on_process("glab",move|_,_|{m.set(true);Ok(ProcessOutput{status:Some(0),stdout:"glab 1.130.0\n".into(),stderr:String::new()})});
        h.on_fetch(move|r|if r.url.split('?').next()==Some("https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7"){let sha=if moved.get(){"ffffffffffffffffffffffffffffffffffffffff"}else{SHA};ok(&MR.replace("\"source_project_id\": 5","\"source_project_id\": 9").replace("\"abc123\"",&format!("\"{sha}\"")))}else{gitlab(r)});
        h.answer_next_prompt(Some(&["ok"]));assert!(p.checkout_source_cli(&h.invocation()).is_err());assert!(h.terminals().is_empty());
    }
}

#[test]
fn draft_note_requests_match_the_documented_gitlab_fields() {
    let host = setup();
    let api = api::Project("g/p");
    let refs = api::DiffRefs { base_sha: "b".into(), head_sha: "h".into(), start_sha: "s".into() };

    api.create_draft(7, "hi", Some(api::diff_position(&refs, "a.rs", "b.rs", None, Some(4))), None).unwrap();
    api.publish_drafts(7).unwrap();

    let posted = writes(&host);
    assert_eq!(posted[0].1, "https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7/draft_notes");
    let body: serde_json::Value = serde_json::from_str(&posted[0].2).unwrap();
    assert_eq!(
        body,
        serde_json::json!({"note": "hi", "position": {"position_type": "text", "base_sha": "b", "start_sha": "s", "head_sha": "h", "old_path": "a.rs", "new_path": "b.rs", "new_line": 4}})
    );
    assert_eq!(posted[1].1, "https://gitlab.com/api/v4/projects/g%2Fp/merge_requests/7/draft_notes/bulk_publish");
}

#[test]
fn thread_marker_icon_is_declared_in_the_manifest() {
    let manifest = include_str!("../plugin.toml");
    assert!(manifest.contains(&format!("icon = \"{}\"", crate::branch::THREAD_MARKER)));
    assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(crate::branch::THREAD_MARKER).is_file());
}

#[test]
fn a_ready_merge_request_without_merge_permission_says_why() {
    let host = setup();
    host.on_fetch(|req| match req.url.split('?').next().is_some_and(|url| url.ends_with("/merge_requests/7")) {
        true => ok(&MR.replace(r#""can_merge": true"#, r#""can_merge": false"#)),
        false => gitlab(req),
    });

    let view = selected(&host).render(DETAILS, None, &host.invocation()).unwrap();

    assert!(!buttons(&view).contains(&"merge"));
    assert!(view.status.as_deref().is_some_and(|status| status.ends_with(" · You cannot merge this merge request")), "{view:?}");
}

#[test]
fn a_repository_without_a_gitlab_remote_offers_share_project() {
    let host = setup();
    host.set_vcs_remotes(&[("origin", "git@github.com:x/y.git")]);

    let view = GitLab::default().render(LIST, None, &host.invocation()).unwrap();

    assert!(buttons(&view).contains(&"share-project"), "{view:?}");
}

mod workspace_selector {
    use super::*;

    /// The window's repository plus an independent `svc-a` child repository on GitLab.
    fn multi_repo() -> MockHost {
        let host = setup();
        host.add_dir("svc-a/.git");
        host.add_file("svc-a/README.md", "x");
        host.on_process("git", |args, _| match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["status", "--porcelain", "-z", "--ignored", "--untracked-files=normal"] => Ok(ProcessOutput { status: Some(0), stdout: String::new(), stderr: String::new() }),
            ["-C", "svc-a", "remote", "-v"] => {
                Ok(ProcessOutput { status: Some(0), stdout: "origin\thttps://gitlab.com/g/svc-a.git (fetch)\n".into(), stderr: String::new() })
            }
            ["-C", "svc-a", "rev-parse", "--abbrev-ref", "HEAD"] => Ok(ProcessOutput { status: Some(0), stdout: "main\n".into(), stderr: String::new() }),
            ["-C", "svc-a", "rev-parse", "HEAD"] => Ok(ProcessOutput { status: Some(0), stdout: "def456\n".into(), stderr: String::new() }),
            _ => Ok(ProcessOutput { status: Some(128), stdout: String::new(), stderr: "unexpected git call".into() }),
        });
        host.on_fetch(|req| {
            let path = req.url.strip_prefix("https://gitlab.com").unwrap_or("");
            match path.split('?').next() {
                Some("/api/v4/projects/g%2Fsvc-a/merge_requests/7") => ok(&MR.replace("g/p", "g/svc-a")),
                Some("/api/v4/projects/g%2Fsvc-a/merge_requests/7/approvals") => {
                    ok(r#"{"approved_by": [], "approvals_left": 1, "user_can_approve": true}"#)
                }
                Some(p) if p.starts_with("/api/v4/projects/g%2Fsvc-a/merge_requests/7/draft_notes") => ok("[]"),
                Some(p) if p.starts_with("/api/v4/projects/g%2Fsvc-a/merge_requests/7/discussions") => ok(DISCUSSIONS),
                Some(p) if p.starts_with("/api/v4/projects/g%2Fsvc-a/merge_requests/7/diffs") => ok(DIFFS),
                Some(p) if p.starts_with("/api/v4/projects/g%2Fsvc-a/merge_requests") => ok(&format!("[{}]", MR.replace("g/p", "g/svc-a"))),
                _ => gitlab(req),
            }
        });
        host
    }

    #[test]
    fn the_repository_button_moves_the_list_to_the_picked_child() {
        let host = multi_repo();
        let mut plugin = GitLab::default();

        let view = plugin.render(LIST, None, &host.invocation()).unwrap();
        assert_eq!(view.status.as_deref(), Some("g/p · Assigned to me"));
        assert!(view.toolbar.iter().any(|button| button.id == "repo"));

        host.answer_next_prompt(Some(&["/project/svc-a"]));
        plugin.handle(LIST, ViewEvent::Button("repo".into()), &host.invocation()).unwrap();

        let view = plugin.render(LIST, None, &host.invocation()).unwrap();
        assert_eq!(view.status.as_deref(), Some("g/svc-a · Assigned to me"));
        assert!(host.requests().iter().any(|request| request.url.contains("projects/g%2Fsvc-a/merge_requests")));
        assert_eq!(workspace::selected("/project").as_deref(), Some("/project/svc-a"));
    }

    #[test]
    fn picking_the_window_again_clears_the_pick() {
        let host = multi_repo();
        workspace::select("/project", Some("/project/svc-a")).unwrap();
        let mut plugin = GitLab::default();
        plugin.render(LIST, None, &host.invocation()).unwrap();

        host.answer_next_prompt(Some(&["/project"]));
        plugin.handle(LIST, ViewEvent::Button("repo".into()), &host.invocation()).unwrap();

        assert_eq!(workspace::selected("/project"), None);
        let view = plugin.render(LIST, None, &host.invocation()).unwrap();
        assert_eq!(view.status.as_deref(), Some("g/p · Assigned to me"));
    }

    #[test]
    fn a_window_repository_without_a_gitlab_remote_follows_its_only_gitlab_child() {
        let host = multi_repo();
        host.set_vcs_remotes(&[("origin", "git@github.com:x/y.git")]);
        let mut plugin = GitLab::default();

        let view = plugin.render(LIST, None, &host.invocation()).unwrap();

        assert_eq!(view.status.as_deref(), Some("g/svc-a · Assigned to me"));
        assert_eq!(workspace::selected("/project"), None, "the child is used without storing a pick");
    }

    #[test]
    fn open_on_gitlab_follows_the_repository_containing_the_file() {
        let host = multi_repo();
        workspace::select("/project", Some("/project/svc-a")).unwrap();
        host.open_editor("/project/svc-a/src/a.rs", "rust", "x", &[]);
        let mut plugin = GitLab::default();

        plugin.run_command("open-in-browser", &host.invocation(), None).unwrap();

        assert_eq!(host.opened_urls(), ["https://gitlab.com/g/svc-a/-/blob/def456/src/a.rs"]);
    }

    #[test]
    fn local_git_actions_ask_for_the_childs_own_window() {
        let host = multi_repo();
        workspace::select("/project", Some("/project/svc-a")).unwrap();
        let mut plugin = selected(&host);

        let view = render_in(&mut plugin, Section::Overview, &host);
        assert!(!buttons(&view).contains(&"checkout"), "the child's branch is not checked out from this window");
        let error = plugin.run_command("create-merge-request", &host.invocation(), None).unwrap_err().to_string();
        assert!(error.contains("own window"), "{error}");
    }

    #[test]
    fn merge_request_commands_never_reuse_a_selection_from_another_repository() {
        let host = multi_repo();
        let mut plugin = selected(&host);
        plugin.update_branch(&host.invocation(), true).unwrap();
        assert!(plugin.branch.as_ref().is_some_and(|branch| branch.mr.is_some()), "the window's branch has a merge request");

        host.answer_next_prompt(Some(&["/project/svc-a"]));
        plugin.handle(LIST, ViewEvent::Button("repo".into()), &host.invocation()).unwrap();

        for command in ["update-merge-request-branch", "edit-merge-request"] {
            host.answer_next_prompt(Some(&[]));
            let error = plugin.run_command(command, &host.invocation(), None).unwrap_err().to_string();
            assert!(error.contains("Select a merge request in the GitLab window first"), "{command}: {error}");
        }
        assert!(!host.requests().iter().any(|request| request.url.contains("g%2Fsvc-a/merge_requests/7")), "svc-a's !7 is never touched");
        assert!(writes(&host).is_empty());
    }

    #[test]
    fn repository_and_refresh_still_work_while_several_repositories_need_a_pick() {
        let host = multi_repo();
        host.set_vcs_remotes(&[("origin", "git@github.com:x/y.git")]);
        host.add_dir("svc-b/.git");
        host.add_file("svc-b/README.md", "x");
        host.on_process("git", |args, _| match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["-C", child, "remote", "-v"] => {
                Ok(ProcessOutput { status: Some(0), stdout: format!("origin\thttps://gitlab.com/g/{child}.git (fetch)\n"), stderr: String::new() })
            }
            _ => Ok(ProcessOutput { status: Some(128), stdout: String::new(), stderr: String::new() }),
        });
        let mut plugin = GitLab::default();

        let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();
        assert!(view.toolbar.iter().any(|button| button.id == "repo"), "{view:?}");
        plugin.handle(DETAILS, ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
        assert!(plugin.handle(LIST, ViewEvent::Button("filter".into()), &host.invocation()).is_err(), "other actions still need a repository");

        host.answer_next_prompt(Some(&["/project/svc-a"]));
        plugin.handle(DETAILS, ViewEvent::Button("repo".into()), &host.invocation()).unwrap();

        let view = plugin.render(LIST, None, &host.invocation()).unwrap();
        assert_eq!(view.status.as_deref(), Some("g/svc-a · Assigned to me"));
    }

    #[test]
    fn changes_open_files_of_the_picked_child_repository_unless_another_project_owns_them() {
        let host = multi_repo();
        workspace::select("/project", Some("/project/svc-a")).unwrap();
        let mut plugin = selected(&host);
        changes_view(&mut plugin, &host).unwrap();
        let open = || ViewEvent::Message(r#"{"type":"open","path":"src/a.rs","line":4}"#.into());

        changes_message(&mut plugin, &host, open()).unwrap();
        assert_eq!(host.opened_locations(), [("/project/svc-a/src/a.rs".into(), 3, 0)]);

        host.add_project("/project/svc-a");
        assert!(changes_message(&mut plugin, &host, open()).is_err(), "a nested project open on its own owns its files");
        assert_eq!(host.opened_locations().len(), 1);
    }

    #[test]
    fn file_links_use_the_commit_checked_out_now() {
        let host = multi_repo();
        let mut plugin = GitLab::default();
        host.open_editor("/project/svc-a/src/a.rs", "rust", "x", &[]);
        plugin.run_command("open-in-browser", &host.invocation(), None).unwrap();
        host.open_editor("/project/README.md", "markdown", "x", &[]);
        plugin.run_command("open-in-browser", &host.invocation(), None).unwrap();

        host.on_process("git", |args, _| match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["-C", "/project/svc-a", "rev-parse", "HEAD"] => Ok(ProcessOutput { status: Some(0), stdout: "0a1b2c\n".into(), stderr: String::new() }),
            _ => Ok(ProcessOutput { status: Some(128), stdout: String::new(), stderr: String::new() }),
        });
        host.set_vcs_status(Some(RepositoryStatus { commit: Some("fff999".into()), ..status("main") }));
        host.open_editor("/project/svc-a/src/a.rs", "rust", "x", &[]);
        plugin.run_command("open-in-browser", &host.invocation(), None).unwrap();
        host.open_editor("/project/README.md", "markdown", "x", &[]);
        plugin.run_command("open-in-browser", &host.invocation(), None).unwrap();

        assert_eq!(
            host.opened_urls(),
            [
                "https://gitlab.com/g/svc-a/-/blob/def456/src/a.rs",
                "https://gitlab.com/g/p/-/blob/abc123/README.md",
                "https://gitlab.com/g/svc-a/-/blob/0a1b2c/src/a.rs",
                "https://gitlab.com/g/p/-/blob/fff999/README.md",
            ]
        );
    }

    #[test]
    fn links_in_the_window_repository_skip_workspace_discovery_without_a_pick() {
        let host = multi_repo();
        host.open_editor("/project/README.md", "markdown", "x", &[]);
        let mut plugin = GitLab::default();

        plugin.run_command("copy-link", &host.invocation(), None).unwrap();
        assert!(host.process_runs().is_empty(), "{:?}", host.process_runs());

        host.select_paths(&["/project/svc-a"]);
        plugin.run_command("open-in-browser", &host.invocation(), None).unwrap();

        assert_eq!(host.clipboard().as_deref(), Some("https://gitlab.com/g/p/-/blob/abc123/README.md"));
        assert_eq!(host.opened_urls(), ["https://gitlab.com/g/svc-a/-/tree/def456/"]);
    }

    #[test]
    fn a_repository_change_refreshes_the_workspace_repositories_without_a_token() {
        let host = multi_repo().with_capabilities(&["vcs.read", "editor.read", "net:gitlab.com", "storage", "project.read", "events", "process:git"]);
        let project = host.invocation().project().unwrap();
        assert_eq!(workspace::repositories(&project, "/project").len(), 2);

        host.set_vcs_remotes(&[("origin", "git@github.com:x/y.git")]);
        GitLab::default().handle_events(&[Event::VcsChanged("/project".into())], &host.invocation());

        assert_eq!(workspace::repositories(&project, "/project").len(), 1, "the window's repository left GitLab");
    }
}
