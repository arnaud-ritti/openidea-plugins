use super::*;
use openidea_plugin::mock::MockHost;
use openidea_plugin::{HttpRequest, HttpResponse, ProcessOutput};

const CAPS: &[&str] = &[
    "vcs.read",
    "editor.read",
    "net:gitlab.com",
    "secrets",
    "storage",
    "clipboard",
    "project.read",
    "events",
    "process:git",
];

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
        ("GET", p) if p.starts_with("/api/v4/projects?membership=true") => ok(
            r#"[{"name": "Tool", "path": "tool", "path_with_namespace": "me/tool", "name_with_namespace": "Me / Tool",
                "http_url_to_repo": "https://gitlab.com/me/tool.git", "ssh_url_to_repo": "git@gitlab.com:me/tool.git"}]"#,
        ),
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
        ("GET", p) if p.starts_with("/api/v4/projects/g%2Fp/repository/branches?") => ok(r#"[{"name": "main"}, {"name": "develop"}, {"name": "feature"}]"#),
        ("GET", p) if p.starts_with("/api/v4/projects/g%2Fp/labels") => ok(r#"[{"name": "bug"}, {"name": "ui"}]"#),
        ("GET", p) if p.starts_with("/api/v4/projects/g%2Fp/members/all") => ok(r#"[{"id": 2, "username": "ann", "name": "Ann"}, {"id": 3, "username": "bob", "name": "Bob"}]"#),
        ("GET", p) if p.starts_with("/api/v4/projects/g%2Fp/milestones") => ok(r#"[{"id": 40, "title": "v2"}]"#),
        ("GET", p) if p.starts_with("/api/v4/namespaces?") => ok(r#"[{"id": 3, "full_path": "me", "kind": "user"}, {"id": 12, "full_path": "team/sub", "kind": "group"}]"#),
        ("GET", "/api/v4/users?username=ann") =>ok(r#"[{"id": 2, "username": "ann"}]"#),
        ("GET", "/api/v4/users?username=bob") => ok(r#"[{"id": 3, "username": "bob"}]"#),
        ("GET", p) if p.starts_with("/api/v4/users?") => ok("[]"),
        ("POST", "/api/v4/projects/g%2Fp/merge_requests") => ok(r#"{"iid": 8, "title": "Add parser", "web_url": "https://gitlab.com/g/p/-/merge_requests/8"}"#),
        ("POST", "/api/v4/snippets") => ok(r#"{"web_url": "https://gitlab.com/-/snippets/9"}"#),
        ("POST", "/api/v4/projects/g%2Fp/snippets") => ok(r#"{"web_url": "https://gitlab.com/g/p/-/snippets/10"}"#),
        ("POST", "/api/v4/projects") => ok(
            r#"{"web_url": "https://gitlab.com/me/project", "http_url_to_repo": "https://gitlab.com/me/project.git",
                "ssh_url_to_repo": "git@gitlab.com:me/project.git"}"#,
        ),
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
    plugin.handle(DETAILS, event, &host.invocation())
}

fn section_buttons(view: &View) -> Vec<(&str, &str)> {
    view.toolbar.iter().filter(|b| b.id.starts_with("section:")).map(|b| (b.id.as_str(), b.label.as_str())).collect()
}

fn field(key: &str, value: &str) -> ViewEvent {
    ViewEvent::Field(key.into(), value.into())
}

fn label<'a>(view: &'a View, id: &str) -> &'a str {
    &view.toolbar.iter().find(|b| b.id == id).unwrap().label
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
    assert_eq!(actions, ["comment", "react", "approve", "merge", "review", "edit", "draft", "close", "checkout", "browser", "copy", "refresh"]);
    assert!(view.toolbar.iter().filter(|b| !b.id.starts_with("section:")).all(|b| b.icon.is_some()), "{view:?}");
    assert_eq!(
        view.status.as_deref(),
        Some("Open · feature → main · ann · Ready to merge · Pipeline passed · Approved by bob · 1 more required")
    );
    let Body::Form(fields) = &view.body else { panic!("{view:?}") };
    let keys: Vec<&str> = fields.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["comment", "as-draft", "emoji", "remove-source-branch", "squash", "description"]);
    assert_eq!(fields[2].value, FieldValue::Choice(safe::EMOJIS.map(str::to_owned).to_vec(), "thumbsup".into()));
    assert_eq!(fields[5].label, "Description (read-only)");
    assert_eq!(fields[5].value, FieldValue::Multiline("Adds it".into()));
    assert!(!fields.iter().any(|f| matches!(f.value, FieldValue::Text(_))), "no read-only facts as inputs: {fields:?}");
}

#[test]
fn a_selected_thread_shows_who_is_replied_to_before_the_reply() {
    let host = setup();
    let mut plugin = GitLab::default();
    plugin.handle(LIST, ViewEvent::Select("d:7:d1".into()), &host.invocation()).unwrap();

    let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();

    let Body::Form(fields) = &view.body else { panic!("{view:?}") };
    assert_eq!(fields[0], Field::new("replying-to", "Replying to (read-only)", FieldValue::Text("bob: Why?".into())));
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
    assert_eq!(fields.last().unwrap().value, FieldValue::Multiline("Adds it".into()));
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
    assert_eq!(label(&view, "merge"), "Confirm Merge !7 into main");

    plugin.handle(DETAILS, ViewEvent::Button("merge".into()), &host.invocation()).unwrap();

    let posted = writes(&host);
    assert_eq!(posted[0].0, "PUT");
    assert!(posted[0].1.ends_with("/merge_requests/7/merge"));
    assert_eq!(posted[0].2, r#"{"sha":"abc123","should_remove_source_branch":true,"squash":true}"#);
    assert!(host.refreshed_views().ends_with(&[DETAILS.into(), LIST.into()]));
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
    assert_eq!(label(&view, "merge"), "Confirm Merge !8 into main");
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
    assert!(host.refreshed_views()[before..].contains(&PIPELINES.to_owned()));
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

    let message = r#"{"type":"comment","oldPath":"src/a.rs","newPath":"src/a.rs","old":4,"new":null,"body":"gone?","draft":false,"anchor":"f0-4-"}"#;
    changes_message(&mut plugin, &host, ViewEvent::Message(message.into())).unwrap();
    let draft = r#"{"type":"comment","oldPath":"src/a.rs","newPath":"src/a.rs","old":3,"new":3,"body":"ctx","draft":true}"#;
    changes_message(&mut plugin, &host, ViewEvent::Message(draft.into())).unwrap();

    let posted = writes(&host);
    assert!(posted[0].1.ends_with("/merge_requests/7/discussions"));
    assert_eq!(
        posted[0].2,
        r#"{"body":"gone?","position":{"base_sha":"abcdef0","head_sha":"abcdef1","new_line":null,"new_path":"src/a.rs","old_line":4,"old_path":"src/a.rs","position_type":"text","start_sha":"start"}}"#
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

    let message = r#"{"type":"comment","oldPath":"src/a.rs","newPath":"src/a.rs","old":4,"new":null,"body":"gone?","draft":false,"anchor":"f0-4-"}"#;
    changes_message(&mut plugin, &host, ViewEvent::Message(message.into())).unwrap();

    assert_eq!(host.refreshed_views().len(), refreshed);
    assert_eq!(plugin.status, None);
    assert_eq!(plugin.anchor, None);
    let (view, posted) = &host.view_messages()[0];
    assert_eq!(view, DETAILS);
    assert!(posted.contains(r#""type":"error""#) && posted.contains("line_code is invalid"), "{posted}");
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

#[test]
fn check_out_without_git_says_so() {
    let without_git: Vec<&str> = CAPS.iter().copied().filter(|c| *c != "process:git").collect();
    let host = setup().with_capabilities(&without_git);
    let mut plugin = selected(&host);

    plugin.handle(DETAILS, ViewEvent::Button("checkout".into()), &host.invocation()).unwrap();

    assert_eq!(plugin.status.as_deref(), Some("Allow git for GitLab in Settings › Plugins to check out branches."));
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
    for (path, url) in [("-x", "https://gitlab.com/me/tool.git"), ("--upload-pack=x", "https://gitlab.com/me/tool.git"), ("tool", "https://gitlab.com/me/tool.git;rm -rf ~")] {
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
    views.push(format!("{:?}", plugin.render(NEW_MR, None, &host.invocation()).unwrap()));
    plugin.handle(NEW_MR, field("title", "Add parser"), &host.invocation()).unwrap();
    plugin.handle(NEW_MR, field("assignee", "ann"), &host.invocation()).unwrap();
    plugin.handle(NEW_MR, ViewEvent::Button("create".into()), &host.invocation()).unwrap();
    views.push(format!("{:?}", plugin.render(NEW_MR, None, &host.invocation()).unwrap()));
    host.open_editor("/project/src/a.rs", "rust", "fn main() {}\n", &[(0, 0, 0, 0)]);
    plugin.handle(NEW_SNIPPET, ViewEvent::Button("create".into()), &host.invocation()).unwrap();
    views.push(format!("{:?}", plugin.render(NEW_SNIPPET, None, &host.invocation()).unwrap()));
    host.set_vcs_remotes(&[("origin", "git@github.com:x/y.git")]);
    GitLab::default().run_command("share-project", &host.invocation(), None).unwrap();

    assert_eq!(api::authorization(), "Bearer {{secret:token}}");
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

    assert_eq!(host.notifications()[0].body, "https://gitlab.com/g/p/-/blob/abc123/a.rs#L1");
}

#[test]
fn creates_a_private_snippet_from_the_selection() {
    let host = setup();
    host.open_editor("/project/src/a.rs", "rust", "fn main() {}\n", &[(0, 3, 0, 7)]);
    host.answer_next_prompt(Some(&[""]));

    GitLab::default().run_command("create-snippet", &host.invocation(), None).unwrap();

    let posted = writes(&host);
    assert_eq!(
        posted[0].2,
        r#"{"files":[{"content":"main","file_path":"src/a.rs"}],"title":"a.rs","visibility":"private"}"#
    );
    assert_eq!(host.clipboard().as_deref(), Some("https://gitlab.com/-/snippets/9"));
    assert_eq!(host.notifications()[0].title, "Snippet created");
}

#[test]
fn create_merge_request_opens_the_new_page_for_the_branch() {
    let host = setup();
    host.set_vcs_status(Some(status("feature")));
    host.set_setting("forms", "false");

    GitLab::default().run_command("create-merge-request", &host.invocation(), None).unwrap();

    assert_eq!(
        host.opened_urls(),
        ["https://gitlab.com/g/p/-/merge_requests/new?merge_request%5Bsource_branch%5D=feature"]
    );
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
    host.on_fetch(|_| ok(&format!("[{}]", vec![r#"{"id": "d"}"#; 100].join(","))));

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
fn files_view_toggles_viewed_without_page_scripts_and_switches_layout() {
    let host = setup();
    host.on_process("git", |_, _| Ok(ProcessOutput { status: Some(128), stdout: String::new(), stderr: "bad object".into() }));
    let mut plugin = selected(&host);
    let items = |plugin: &mut GitLab| {
        let view = plugin.render(FILES, None, &host.invocation()).unwrap();
        let Body::Tree(items) = view.body else { panic!("{view:?}") };

        items
    };

    let tree = items(&mut plugin);
    assert_eq!(tree.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["dir:src", "file:src/a.rs"]);
    assert_eq!(tree[1].parent.as_deref(), Some("dir:src"));
    assert!(tree[1].icon.is_none());

    plugin.handle(FILES, ViewEvent::Activate("file:src/a.rs".into()), &host.invocation()).unwrap();

    assert_eq!(storage::get("viewed:g/p:7").unwrap().unwrap(), r#"{"src/a.rs":"abcdef1"}"#);
    assert_eq!(items(&mut plugin)[1].icon.as_deref(), Some("check"));
    assert!(changes_html(&mut plugin, &host).contains(" checked"));

    plugin.handle(FILES, ViewEvent::Button("layout".into()), &host.invocation()).unwrap();
    let list = items(&mut plugin);
    assert_eq!((list.len(), list[0].label.as_str(), list[0].parent.as_deref()), (1, "src/a.rs", None));

    plugin.handle(FILES, ViewEvent::Activate("file:nope".into()), &host.invocation()).unwrap();
    plugin.handle(FILES, ViewEvent::Button("clear".into()), &host.invocation()).unwrap();
    assert_eq!(storage::get("viewed:g/p:7").unwrap().unwrap(), "{}");

    plugin.handle(FILES, ViewEvent::Button("mark-all".into()), &host.invocation()).unwrap();
    assert_eq!(storage::get("viewed:g/p:7").unwrap().unwrap(), r#"{"src/a.rs":"abcdef1"}"#);
}

#[test]
fn viewed_falls_back_to_the_head_without_local_history() {
    let host = setup();
    host.on_process("git", |_, _| Ok(ProcessOutput { status: Some(128), stdout: String::new(), stderr: "bad object".into() }));
    let mut plugin = selected(&host);

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

    let edits: Vec<_> = actions.iter().map(|a| (a.edits[0].1[0].range.start.line, a.edits[0].1[0].range.end.line, a.edits[0].1[0].text.as_str())).collect();
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
    host.on_fetch(|_| ok(&format!("[{}]", vec![r#"{"id": "d"}"#; 100].join(","))));
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
    plugin.diffs = Some((7, vec![FileDiff {
        old_path: "src/a.rs".into(),
        new_path: "src/a.rs".into(),
        diff: "@@ -3,2 +3,2 @@\n c\n+d\n".into(),
        ..FileDiff::default()
    }]));

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
    plugin.diffs = Some((7, vec![FileDiff {
        old_path: "src/a.rs".into(),
        new_path: "src/a.rs".into(),
        diff: "@@ -3,2 +3,2 @@\n c\n+d\n".into(),
        ..FileDiff::default()
    }]));

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
    host.on_fetch(|_| ok(&format!("[{}]", vec![r#"{"id": "d"}"#; 100].join(","))));
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
        let stale = openidea_plugin::Diagnostic::new(Range::new(Position::new(0, 0), Position::new(0, 1)), openidea_plugin::Severity::Warning, "old");
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
        let yaml = "job:\n  script:\n    - echo 'deploying $CI_COMMIT_SHA'\n    - printf '${HOME}\\n'\n  after_script:\n    - echo -n '$X'\n";
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
        for path in [".gitlab-ci.yml", "/p/.gitlab-ci.yaml", "/p/team.gitlab-ci.yml", "x.gitlab-ci.yaml", "/p/a.gitlab-ci.deploy.yml", "a.gitlab-ci.b.yaml"] {
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
            [
                "fetch -- origin +refs/heads/feature:refs/remotes/origin/feature",
                "worktree add --detach -- /project-feature origin/feature",
            ]
        );
        assert!(host.notifications()[0].body.contains("/project-feature"));
    }

    #[test]
    fn worktree_without_git_notifies() {
        let without_git: Vec<&str> = CAPS.iter().copied().filter(|c| *c != "process:git").collect();
        let host = setup().with_capabilities(&without_git);
        let _plugin = selected(&host);

        GitLab::default().run_command("open-merge-request-in-worktree", &host.invocation(), None).unwrap();

        assert!(host.notifications()[0].body.contains("Allow git"));
    }

    #[test]
    fn update_branch_request_targets_rebase_endpoint() {
        let host = setup();
        let _plugin = selected(&host);

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

        let error = GitLab::default().run_command("update-merge-request-branch", &host.invocation(), None).unwrap_err();

        assert_eq!(error.to_string(), "Rebase of !7 failed: Rebase failed. Please rebase locally");
    }
}

mod merge_request_form_tests {
    use super::*;
    use serde_json::Value;

    fn on_feature() -> MockHost {
        let host = setup();
        host.set_vcs_status(Some(status("feature")));
        host
    }

    fn open(host: &MockHost) -> (GitLab, View) {
        let mut plugin = GitLab::default();
        let view = plugin.render(NEW_MR, None, &host.invocation()).unwrap();
        (plugin, view)
    }

    fn fill(plugin: &mut GitLab, host: &MockHost, fields: &[(&str, &str)]) {
        for (key, value) in fields {
            plugin.handle(NEW_MR, field(key, value), &host.invocation()).unwrap();
        }
    }

    fn create(plugin: &mut GitLab, host: &MockHost) -> View {
        plugin.handle(NEW_MR, ViewEvent::Button("create".into()), &host.invocation()).unwrap();
        plugin.render(NEW_MR, None, &host.invocation()).unwrap()
    }

    fn created(host: &MockHost) -> Vec<Value> {
        writes(host)
            .into_iter()
            .filter(|(method, url, _)| method == "POST" && url == "https://gitlab.com/api/v4/projects/g%2Fp/merge_requests")
            .map(|(_, _, body)| serde_json::from_str(&body).unwrap())
            .collect()
    }

    fn value<'a>(view: &'a View, key: &str) -> &'a FieldValue {
        let Body::Form(fields) = &view.body else { panic!("{view:?}") };
        &fields.iter().find(|f| f.key == key).unwrap().value
    }

    #[test]
    fn form_lists_the_branches_and_defaults_the_target_to_the_project_default() {
        let host = on_feature();

        let (_, view) = open(&host);

        let branches = vec!["main".to_owned(), "develop".to_owned(), "feature".to_owned()];
        assert_eq!(value(&view, "target-branch"), &FieldValue::Choice(branches, "main".into()));
        assert_eq!(view.status.as_deref(), Some("feature → main"));
        assert_eq!(buttons(&view), ["create", "refresh"]);
        assert!(host.requests().iter().any(|r| r.url.contains("/repository/branches?per_page=100&page=1")));
    }

    #[test]
    fn create_posts_title_source_target_and_options() {
        let host = on_feature();
        let (mut plugin, _) = open(&host);
        fill(
            &mut plugin,
            &host,
            &[
                ("title", "Add parser"),
                ("description", "Adds it"),
                ("target-branch", "develop"),
                ("source-branch", "evil"),
                ("squash", "true"),
                ("remove-source-branch", "true"),
            ],
        );

        create(&mut plugin, &host);

        let body = &created(&host)[0];
        assert_eq!(body["source_branch"], "feature");
        assert_eq!(body["target_branch"], "develop");
        assert_eq!(body["title"], "Add parser");
        assert_eq!(body["description"], "Adds it");
        assert_eq!(body["squash"], true);
        assert_eq!(body["remove_source_branch"], true);
        assert_eq!(body["labels"], "");
        assert_eq!(body["reviewer_ids"], serde_json::json!([]));
        assert!(body.get("assignee_id").is_none());
    }

    #[test]
    fn draft_prefixes_the_title_with_draft() {
        let host = on_feature();
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("title", "Add parser"), ("draft", "true")]);

        create(&mut plugin, &host);

        assert_eq!(created(&host)[0]["title"], "Draft: Add parser");
    }

    #[test]
    fn assignee_and_reviewers_are_resolved_by_username_lookup() {
        let host = on_feature();
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("title", "Add parser"), ("assignee", "ann"), ("reviewers", "ann, @bob")]);

        create(&mut plugin, &host);

        let body = &created(&host)[0];
        assert_eq!(body["assignee_id"], 2);
        assert_eq!(body["reviewer_ids"], serde_json::json!([2, 3]));
        assert!(host.requests().iter().any(|r| r.url == "https://gitlab.com/api/v4/users?username=bob"));
    }

    #[test]
    fn an_unknown_username_fails_and_creates_nothing() {
        let host = on_feature();
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("title", "Add parser"), ("reviewers", "ann ghost")]);

        let view = create(&mut plugin, &host);

        assert!(writes(&host).is_empty());
        assert!(view.status.as_deref().unwrap().contains("ghost"), "{view:?}");
    }

    #[test]
    fn usernames_with_url_metacharacters_are_refused_before_any_request() {
        for (key, name) in [("assignee", "a&b#c/d"), ("assignee", "-x"), ("reviewers", "ann, a&b#c/d"), ("reviewers", "-x")] {
            let host = on_feature();
            let (mut plugin, _) = open(&host);
            fill(&mut plugin, &host, &[("title", "Add parser"), (key, name)]);

            let view = create(&mut plugin, &host);

            assert!(host.requests().iter().all(|r| !r.url.contains("/users")), "{name}");
            assert!(writes(&host).is_empty(), "{name}");
            assert!(view.status.as_deref().unwrap().contains("refusing the username"), "{view:?}");
        }
    }

    #[test]
    fn a_target_branch_outside_the_listed_branches_is_ignored() {
        let host = on_feature();
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("title", "Add parser"), ("target-branch", "develop"), ("target-branch", "main\"}, \"x"), ("target-branch", "--evil")]);

        let view = plugin.render(NEW_MR, None, &host.invocation()).unwrap();
        create(&mut plugin, &host);

        let FieldValue::Choice(_, chosen) = value(&view, "target-branch") else { panic!("{view:?}") };
        assert_eq!(chosen, "develop");
        assert_eq!(created(&host)[0]["target_branch"], "develop");
    }

    #[test]
    fn labels_are_trimmed_and_empty_ones_dropped() {
        let host = on_feature();
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("title", "Add parser"), ("labels", " bug , ,ui ,,")]);

        create(&mut plugin, &host);

        assert_eq!(created(&host)[0]["labels"], "bug,ui");
    }

    #[test]
    fn a_detached_head_refuses_and_says_so() {
        let host = setup();
        host.set_vcs_status(Some(RepositoryStatus { branch: None, ..status("main") }));
        let (mut plugin, view) = open(&host);
        fill(&mut plugin, &host, &[("title", "Add parser")]);

        create(&mut plugin, &host);

        assert_eq!(view.body, Body::Message(DETACHED.into()));
        assert!(writes(&host).is_empty());
    }

    #[test]
    fn an_empty_title_is_refused() {
        let host = on_feature();
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("title", "   ")]);

        let view = create(&mut plugin, &host);

        assert!(writes(&host).is_empty());
        assert_eq!(view.status.as_deref(), Some("Write a title first. · feature → main"));
    }

    #[test]
    fn creating_reports_the_web_url_in_a_notification_and_refreshes_the_list() {
        let host = on_feature();
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("title", "Add parser")]);

        let view = create(&mut plugin, &host);

        let notification = &host.notifications()[0];
        assert_eq!(notification.title, "Merge request created");
        assert!(notification.body.contains("https://gitlab.com/g/p/-/merge_requests/8"), "{notification:?}");
        assert!(host.refreshed_views().iter().any(|v| v == LIST));
        assert_eq!(value(&view, "title"), &FieldValue::Text(String::new()));

        create(&mut plugin, &host);
        assert_eq!(created(&host).len(), 1, "a second click on the emptied form creates nothing");
    }

    #[test]
    fn the_form_is_quiet_when_forms_setting_is_off() {
        let host = on_feature();
        host.set_setting("forms", "false");

        let (mut plugin, view) = open(&host);
        fill(&mut plugin, &host, &[("title", "Add parser")]);
        create(&mut plugin, &host);

        assert!(matches!(view.body, Body::Message(_)), "{view:?}");
        assert!(host.requests().is_empty());
    }

    #[test]
    fn without_a_token_the_form_asks_for_one() {
        let host = MockHost::install().with_capabilities(CAPS);
        host.set_vcs_status(Some(status("feature")));

        let (_, view) = open(&host);

        assert_eq!(view.body, Body::Message(NO_TOKEN.into()));
        assert!(host.requests().is_empty());
    }
}

mod snippet_form_tests {
    use super::*;
    use serde_json::Value;

    fn open(host: &MockHost) -> (GitLab, View) {
        let mut plugin = GitLab::default();
        let view = plugin.render(NEW_SNIPPET, None, &host.invocation()).unwrap();
        (plugin, view)
    }

    fn fill(plugin: &mut GitLab, host: &MockHost, fields: &[(&str, &str)]) {
        for (key, value) in fields {
            plugin.handle(NEW_SNIPPET, field(key, value), &host.invocation()).unwrap();
        }
    }

    fn click(plugin: &mut GitLab, host: &MockHost, id: &str) -> View {
        plugin.handle(NEW_SNIPPET, ViewEvent::Button(id.into()), &host.invocation()).unwrap();
        plugin.render(NEW_SNIPPET, None, &host.invocation()).unwrap()
    }

    fn posted(host: &MockHost) -> Vec<(String, Value)> {
        writes(host).into_iter().map(|(_, url, body)| (url, serde_json::from_str(&body).unwrap())).collect()
    }

    fn with_file(text: &str, selection: (u32, u32, u32, u32)) -> MockHost {
        let host = setup();
        host.open_editor("/project/src/a.rs", "rust", text, &[selection]);
        host
    }

    #[test]
    fn form_creates_a_private_personal_snippet_from_the_selection() {
        let host = with_file("fn main() {}\n", (0, 3, 0, 7));
        let (mut plugin, view) = open(&host);
        fill(&mut plugin, &host, &[("description", "The entry point")]);

        click(&mut plugin, &host, "create");

        assert_eq!(buttons(&view), ["create"]);
        let posted = writes(&host);
        assert_eq!(posted[0].1, "https://gitlab.com/api/v4/snippets");
        assert_eq!(
            posted[0].2,
            r#"{"description":"The entry point","files":[{"content":"main","file_path":"src/a.rs"}],"title":"a.rs","visibility":"private"}"#
        );
        assert_eq!(host.clipboard().as_deref(), Some("https://gitlab.com/-/snippets/9"));
        assert_eq!(host.notifications()[0].title, "Snippet created");
    }

    #[test]
    fn form_adds_more_project_files_to_the_snippet() {
        let host = with_file("x\n", (0, 0, 0, 0));
        host.add_file("docs/b.md", "second\n");
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("more-files", "docs/b.md\n")]);

        click(&mut plugin, &host, "create");

        let posted = posted(&host);
        assert_eq!(posted[0].1["files"][0]["file_path"], "src/a.rs");
        assert_eq!(posted[0].1["files"][1]["file_path"], "docs/b.md");
        assert_eq!(posted[0].1["files"][1]["content"], "second\n");
    }

    #[test]
    fn project_scope_posts_to_the_encoded_project_path() {
        let host = with_file("x\n", (0, 0, 0, 0));
        let (mut plugin, view) = open(&host);
        let Body::Form(fields) = &view.body else { panic!("{view:?}") };
        let scope = &fields.iter().find(|f| f.key == "scope").unwrap().value;
        assert_eq!(scope, &FieldValue::Choice(vec!["Personal".into(), "Project g/p".into()], "Personal".into()));
        fill(&mut plugin, &host, &[("title", "Mine"), ("file-path", "notes/x.txt"), ("scope", "Project g/p")]);

        click(&mut plugin, &host, "create");

        let posted = posted(&host);
        assert_eq!(posted[0].0, "https://gitlab.com/api/v4/projects/g%2Fp/snippets");
        assert_eq!(posted[0].1["title"], "Mine");
        assert_eq!(posted[0].1["files"][0]["file_path"], "notes/x.txt");
    }

    #[test]
    fn public_visibility_needs_a_second_click() {
        let host = with_file("x\n", (0, 0, 0, 0));
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("visibility", "public")]);

        let view = click(&mut plugin, &host, "create");

        assert!(writes(&host).is_empty());
        assert_eq!(label(&view, "confirm-public"), "Confirm public");

        click(&mut plugin, &host, "confirm-public");

        let posted = posted(&host);
        assert_eq!(posted.len(), 1);
        assert_eq!(posted[0].1["visibility"], "public");
    }

    #[test]
    fn a_focus_change_after_the_public_warning_publishes_nothing() {
        let host = with_file("a\n", (0, 0, 0, 0));
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("visibility", "public")]);
        let warned = click(&mut plugin, &host, "create");
        assert!(warned.status.as_deref().unwrap().contains("src/a.rs"), "{warned:?}");

        host.open_editor("/project/.env", "dotenv", "TOKEN=secret\n", &[(0, 0, 0, 0)]);
        let view = click(&mut plugin, &host, "confirm-public");

        assert!(writes(&host).is_empty());
        assert_eq!(buttons(&view), ["create"]);
        assert!(view.status.as_deref().unwrap().contains("changed"), "{view:?}");

        click(&mut plugin, &host, "create");
        click(&mut plugin, &host, "confirm-public");
        let posted = posted(&host);
        assert_eq!(posted.len(), 1);
        assert_eq!(posted[0].1["files"][0]["file_path"], ".env");
    }

    #[test]
    fn changing_any_field_cancels_the_pending_public_confirm() {
        let host = with_file("x\n", (0, 0, 0, 0));
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("visibility", "public")]);
        click(&mut plugin, &host, "create");

        fill(&mut plugin, &host, &[("title", "Other")]);
        let view = click(&mut plugin, &host, "confirm-public");

        assert!(writes(&host).is_empty());
        assert_eq!(buttons(&view), ["create"]);
    }

    #[test]
    fn a_visibility_outside_the_choices_is_ignored() {
        let host = with_file("x\n", (0, 0, 0, 0));
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("visibility", "internal"), ("visibility", "everyone"), ("scope", "Project evil/x")]);

        click(&mut plugin, &host, "create");

        let posted = posted(&host);
        assert_eq!(posted[0].0, "https://gitlab.com/api/v4/snippets");
        assert_eq!(posted[0].1["visibility"], "internal");
    }

    #[test]
    fn file_path_traversal_is_refused() {
        for path in ["../../x", "/etc/x"] {
            let host = with_file("x\n", (0, 0, 0, 0));
            let (mut plugin, _) = open(&host);
            fill(&mut plugin, &host, &[("file-path", path)]);

            let view = click(&mut plugin, &host, "create");

            assert!(writes(&host).is_empty(), "{path}");
            assert!(view.status.as_deref().unwrap().contains("refusing the file path"), "{view:?}");
        }
    }

    #[test]
    fn no_focused_file_says_focus_a_file_first() {
        let host = setup();
        let (mut plugin, _) = open(&host);

        let view = click(&mut plugin, &host, "create");

        assert!(writes(&host).is_empty());
        assert_eq!(view.status.as_deref(), Some("Focus a file first."));
    }

    #[test]
    fn an_empty_selection_falls_back_to_the_whole_file_and_an_empty_file_is_refused() {
        let host = with_file("a\nb\n", (1, 0, 1, 0));
        let (mut plugin, _) = open(&host);
        click(&mut plugin, &host, "create");
        assert_eq!(posted(&host)[0].1["files"][0]["content"], "a\nb\n");

        let host = with_file("", (0, 0, 0, 0));
        let (mut plugin, _) = open(&host);
        let view = click(&mut plugin, &host, "create");
        assert!(writes(&host).is_empty());
        assert_eq!(view.status.as_deref(), Some("The file is empty."));
    }

    #[test]
    fn crlf_content_is_sent_unchanged() {
        let host = with_file("a\r\nb\r\n", (0, 0, 0, 0));
        let (mut plugin, _) = open(&host);

        click(&mut plugin, &host, "create");

        assert_eq!(posted(&host)[0].1["files"][0]["content"], "a\r\nb\r\n");
    }

    #[test]
    fn multibyte_selection_is_cut_on_utf16_columns() {
        let host = with_file("a😀b", (0, 3, 0, 5));
        let (mut plugin, _) = open(&host);

        click(&mut plugin, &host, "create");

        assert_eq!(posted(&host)[0].1["files"][0]["content"], "b");
    }

    #[test]
    fn the_form_is_quiet_when_forms_setting_is_off() {
        let host = with_file("x\n", (0, 0, 0, 0));
        host.set_setting("forms", "false");

        let (mut plugin, view) = open(&host);
        click(&mut plugin, &host, "create");

        assert!(matches!(view.body, Body::Message(_)), "{view:?}");
        assert!(host.requests().is_empty());
    }
}

#[test]
fn check_out_refuses_a_merge_request_branch_that_reads_as_an_option() {
    let host = setup();
    host.on_process("git", git_ok);
    host.on_fetch(|r| match r.url.contains("/merge_requests/7") && !r.url.contains("/merge_requests/7/") {
        true => ok(&MR.replace(r#""source_branch": "feature""#, r#""source_branch": "--upload-pack=x""#)),
        false => gitlab(r),
    });
    let mut plugin = selected(&host);

    plugin.handle(DETAILS, ViewEvent::Button("checkout".into()), &host.invocation()).unwrap();

    assert!(host.process_runs().is_empty(), "{:?}", git_runs(&host));
}

mod share_form_tests {
    use super::*;
    use serde_json::Value;

    fn git_staging(files: &'static str, staged: String, failing: &'static str) -> impl Fn(&[String], Option<&str>) -> Result<ProcessOutput, Error> {
        move |args, _| {
            let stdout = match args[0].as_str() {
                "status" => files.replace('\n', "\0"),
                "diff" => staged.clone(),
                "symbolic-ref" => "main\n".into(),
                _ => String::new(),
            };
            let status = if args[0] == failing { 1 } else { 0 };
            Ok(ProcessOutput { status: Some(status), stdout, stderr: "boom".into() })
        }
    }

    fn git_with(files: &'static str, failing: &'static str) -> impl Fn(&[String], Option<&str>) -> Result<ProcessOutput, Error> {
        git_staging(files, files.lines().map(|l| format!("{}\0", &l[3..])).collect(), failing)
    }

    fn unshared() -> MockHost {
        let host = setup();
        host.set_vcs_remotes(&[]);
        host.on_process("git", git_ok);
        host
    }

    fn open(host: &MockHost) -> (GitLab, View) {
        let mut plugin = GitLab::default();
        let view = plugin.render(SHARE, None, &host.invocation()).unwrap();
        (plugin, view)
    }

    fn fill(plugin: &mut GitLab, host: &MockHost, fields: &[(&str, &str)]) {
        for (key, value) in fields {
            plugin.handle(SHARE, field(key, value), &host.invocation()).unwrap();
        }
    }

    fn click(plugin: &mut GitLab, host: &MockHost, id: &str) -> View {
        plugin.handle(SHARE, ViewEvent::Button(id.into()), &host.invocation()).unwrap();
        plugin.render(SHARE, None, &host.invocation()).unwrap()
    }

    fn created(host: &MockHost) -> Vec<Value> {
        writes(host).into_iter().filter(|(_, url, _)| url == "https://gitlab.com/api/v4/projects").map(|(_, _, body)| serde_json::from_str(&body).unwrap()).collect()
    }

    fn value<'a>(view: &'a View, key: &str) -> &'a FieldValue {
        let Body::Form(fields) = &view.body else { panic!("{view:?}") };

        &fields.iter().find(|f| f.key == key).unwrap().value
    }

    fn nothing_happened(host: &MockHost) {
        assert!(writes(host).is_empty(), "{:?}", writes(host));
        assert!(host.process_runs().is_empty(), "{:?}", git_runs(host));
        assert!(host.terminals().is_empty());
    }

    #[test]
    fn share_form_creates_in_the_chosen_namespace_with_description_and_visibility() {
        let host = unshared();
        let (mut plugin, view) = open(&host);
        fill(&mut plugin, &host, &[("name", "tool"), ("namespace", "team/sub"), ("description", "A tool"), ("visibility", "internal")]);

        click(&mut plugin, &host, "share");

        assert_eq!(value(&view, "namespace"), &FieldValue::Choice(vec!["Personal".into(), "me".into(), "team/sub".into()], "Personal".into()));
        assert!(host.requests().iter().any(|r| r.url == "https://gitlab.com/api/v4/namespaces?per_page=100&page=1"));
        assert_eq!(created(&host), [serde_json::json!({"name": "tool", "namespace_id": 12, "description": "A tool", "visibility": "internal"})]);
        assert_eq!(host.notifications()[0].body, "https://gitlab.com/me/project");
    }

    #[test]
    fn remote_names_starting_with_a_dash_are_refused_before_any_request() {
        for remote in ["-u", "--mirror"] {
            let host = unshared();
            let (mut plugin, _) = open(&host);
            fill(&mut plugin, &host, &[("remote", remote)]);

            let view = click(&mut plugin, &host, "share");

            nothing_happened(&host);
            assert!(view.status.unwrap_or_default().contains("refusing"), "{remote}");
        }
    }

    #[test]
    fn remote_name_with_shell_metacharacters_is_refused() {
        for remote in ["a;b", "a b", "$(x)"] {
            let host = unshared();
            let (mut plugin, _) = open(&host);
            fill(&mut plugin, &host, &[("remote", remote)]);

            click(&mut plugin, &host, "share");

            nothing_happened(&host);
        }
    }

    #[test]
    fn a_branch_with_shell_metacharacters_is_refused_before_pushing() {
        let host = unshared();
        host.set_vcs_status(Some(status("x;rm -rf ~")));
        let (mut plugin, _) = open(&host);

        click(&mut plugin, &host, "share");

        nothing_happened(&host);
        assert!(created(&host).is_empty());
    }

    #[test]
    fn remote_add_and_push_put_names_after_double_dash() {
        let host = unshared();
        let (mut plugin, _) = open(&host);

        click(&mut plugin, &host, "share");

        assert_eq!(created(&host), [serde_json::json!({"name": "project", "visibility": "private"})]);
        assert_eq!(git_runs(&host), ["--version", "remote add -- origin https://gitlab.com/me/project.git"]);
        assert_eq!(host.terminals()[0].args, ["push", "-u", "--", "origin", "main"]);
    }

    #[test]
    fn a_returned_clone_url_on_another_host_is_refused_and_not_added() {
        let host = unshared();
        host.on_fetch(|r| match r.method == "POST" && r.url.ends_with("/api/v4/projects") {
            true => ok(r#"{"web_url": "https://gitlab.com/me/project", "http_url_to_repo": "https://evil.test/x.git", "ssh_url_to_repo": "git@evil.test:x.git"}"#),
            false => gitlab(r),
        });
        let (mut plugin, _) = open(&host);

        let view = click(&mut plugin, &host, "share");

        assert_eq!(git_runs(&host), ["--version"]);
        assert!(host.terminals().is_empty());
        assert!(view.status.unwrap_or_default().contains("refusing the clone URL"));
    }

    #[test]
    fn init_runs_only_when_there_is_no_repository_and_the_box_is_ticked() {
        let host = unshared();
        host.set_vcs_status(None);
        host.on_process("git", git_with("?? src/a.rs\n", ""));
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("commit-all", "true")]);

        click(&mut plugin, &host, "share");
        assert!(host.process_runs().is_empty(), "{:?}", git_runs(&host));

        fill(&mut plugin, &host, &[("init", "true")]);
        click(&mut plugin, &host, "share");
        assert_eq!(git_runs(&host), ["--version", "init", "symbolic-ref --short HEAD", "status --porcelain -z --untracked-files=all"]);

        let host = unshared();
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("init", "true")]);
        click(&mut plugin, &host, "share");
        assert!(!git_runs(&host).contains(&"init".to_owned()), "{:?}", git_runs(&host));
        assert_eq!(created(&host).len(), 1);
    }

    #[test]
    fn commit_all_lists_files_and_waits_for_the_second_click() {
        let host = unshared();
        host.on_process("git", git_with("?? src/a.rs\n M README.md\n", ""));
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("commit-all", "true")]);

        let view = click(&mut plugin, &host, "share");

        assert_eq!(buttons(&view), ["confirm-commit", "refresh"]);
        assert!(view.status.as_deref().unwrap_or_default().contains("2 files"), "{:?}", view.status);
        assert_eq!(value(&view, "files"), &FieldValue::Multiline("src/a.rs\nREADME.md".into()));
        assert!(!git_runs(&host).iter().any(|r| r.starts_with("add")));
        assert!(writes(&host).is_empty());

        click(&mut plugin, &host, "confirm-commit");

        assert_eq!(
            git_runs(&host)[3..],
            [
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
    fn a_file_staged_after_the_confirm_is_unstaged_and_listed_again() {
        let host = unshared();
        host.on_process("git", git_with("?? src/a.rs\n", ""));
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("commit-all", "true")]);
        click(&mut plugin, &host, "share");

        host.on_process("git", git_staging("?? src/a.rs\n", "src/a.rs\0notes.txt\0".into(), ""));
        let view = click(&mut plugin, &host, "confirm-commit");

        let runs = git_runs(&host);
        assert!(runs.contains(&"reset -q".to_owned()), "{runs:?}");
        assert!(!runs.iter().any(|r| r.starts_with("commit") || r.starts_with("remote")), "{runs:?}");
        assert!(created(&host).is_empty());
        assert!(host.terminals().is_empty());
        assert_eq!(buttons(&view), ["confirm-commit", "refresh"]);
        assert_eq!(value(&view, "files"), &FieldValue::Multiline("src/a.rs\nnotes.txt".into()));
    }

    #[test]
    fn porcelain_renames_list_both_sides_and_order_does_not_matter_for_the_confirm() {
        let host = unshared();
        host.on_process("git", git_staging("R  new.rs\nold.rs\n?? b.rs\n", "b.rs\0new.rs\0old.rs\0".into(), ""));
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("commit-all", "true")]);

        let view = click(&mut plugin, &host, "share");
        assert_eq!(value(&view, "files"), &FieldValue::Multiline("new.rs\nold.rs\nb.rs".into()));

        click(&mut plugin, &host, "confirm-commit");

        assert!(git_runs(&host).contains(&"commit -m Initial commit".to_owned()));
        assert_eq!(created(&host).len(), 1);
    }

    #[test]
    fn the_commit_confirm_is_bound_to_the_files_it_listed() {
        let host = unshared();
        host.on_process("git", git_with("?? src/a.rs\n", ""));
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("commit-all", "true")]);
        click(&mut plugin, &host, "share");

        host.on_process("git", git_with("?? src/a.rs\n?? notes.txt\n", ""));
        let view = click(&mut plugin, &host, "confirm-commit");

        assert!(!git_runs(&host).iter().any(|r| r.starts_with("add")));
        assert!(writes(&host).is_empty());
        assert_eq!(buttons(&view), ["confirm-commit", "refresh"]);
        assert!(view.status.unwrap_or_default().contains("changed"));
    }

    #[test]
    fn changing_a_field_cancels_the_pending_commit_confirm() {
        let host = unshared();
        host.on_process("git", git_with("?? src/a.rs\n", ""));
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("commit-all", "true")]);
        click(&mut plugin, &host, "share");

        fill(&mut plugin, &host, &[("remote", "gitlab")]);
        let view = click(&mut plugin, &host, "confirm-commit");

        assert!(!git_runs(&host).iter().any(|r| r.starts_with("add")));
        assert_eq!(buttons(&view), ["share", "refresh"]);
    }

    #[test]
    fn commit_all_refuses_secret_looking_files() {
        for file in [".env", "deploy.key", "id_rsa", "config/.env.local", "certs/site.PEM", ".ssh/id_ed25519.pub"] {
            let host = unshared();
            host.on_process("git", git_with(Box::leak(format!("?? src/a.rs\n?? {file}\n").into_boxed_str()), ""));
            let (mut plugin, _) = open(&host);
            fill(&mut plugin, &host, &[("commit-all", "true")]);

            let view = click(&mut plugin, &host, "share");

            assert!(!git_runs(&host).iter().any(|r| r.starts_with("add")), "{file}");
            assert!(writes(&host).is_empty(), "{file}");
            assert_eq!(buttons(&view), ["share", "refresh"], "{file}");
            assert!(view.status.unwrap_or_default().contains(file), "{file}");
        }
    }

    #[test]
    fn nothing_is_created_when_git_init_or_commit_fails() {
        let host = unshared();
        host.set_vcs_status(None);
        host.on_process("git", git_with("?? src/a.rs\n", "init"));
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("init", "true"), ("commit-all", "true")]);
        let view = click(&mut plugin, &host, "share");
        assert!(writes(&host).is_empty());
        assert!(view.status.unwrap_or_default().contains("Git init failed"));

        let host = unshared();
        host.on_process("git", git_with("?? src/a.rs\n", "commit"));
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("commit-all", "true")]);
        click(&mut plugin, &host, "share");
        click(&mut plugin, &host, "confirm-commit");
        assert!(writes(&host).is_empty());
        assert!(host.terminals().is_empty());
    }

    #[test]
    fn an_existing_gitlab_remote_refuses() {
        let host = setup();
        host.on_process("git", git_ok);
        let (mut plugin, _) = open(&host);

        let view = click(&mut plugin, &host, "share");

        nothing_happened(&host);
        assert!(view.status.unwrap_or_default().contains("already has a gitlab.com remote"));

        host.set_vcs_remotes(&[("upstream", "git@github.com:x/y.git")]);
        fill(&mut plugin, &host, &[("remote", "upstream")]);
        click(&mut plugin, &host, "share");
        nothing_happened(&host);
    }

    #[test]
    fn share_without_process_git_names_the_capability() {
        let without_git: Vec<&str> = CAPS.iter().copied().filter(|c| *c != "process:git").collect();
        let host = unshared().with_capabilities(&without_git);

        let result = GitLab::default().run_command("share-project", &host.invocation(), None);

        assert_eq!(result, Err(Error::Denied("process:git".into())));
        assert!(writes(&host).is_empty());
    }

    #[test]
    fn a_namespace_outside_the_choices_is_ignored() {
        let host = unshared();
        let (mut plugin, _) = open(&host);
        fill(&mut plugin, &host, &[("namespace", "evil/ns"), ("visibility", "secret")]);

        let view = click(&mut plugin, &host, "share");

        assert_eq!(value(&view, "namespace"), &FieldValue::Choice(vec!["Personal".into(), "me".into(), "team/sub".into()], "Personal".into()));
        assert_eq!(created(&host), [serde_json::json!({"name": "project", "visibility": "private"})]);
    }

    #[test]
    fn the_form_is_quiet_when_forms_setting_is_off() {
        let host = unshared();
        host.set_setting("forms", "false");

        let (mut plugin, view) = open(&host);
        click(&mut plugin, &host, "share");

        assert!(matches!(view.body, Body::Message(_)), "{view:?}");
        assert!(host.requests().is_empty());
        assert!(host.process_runs().is_empty());
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
            ("GET", "/notes/12/award_emoji") => ok(
                r#"[{"id": 55, "name": "thumbsup", "user": {"id": 1, "username": "me"}},
                    {"id": 56, "name": "rocket", "user": {"id": 3, "username": "bob"}}]"#,
            ),
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
                ("GET", "/discussions") => ok(r#"[{"id": "d1", "notes": [{"id": 12, "body": "x", "author": {"username": "me"}, "created_at": "2026-01-03"}]}]"#),
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
        plugin.render(COMMITS, None, &host.invocation()).unwrap();
        plugin
    }

    fn pick(plugin: &mut GitLab, host: &MockHost, key: &str) {
        plugin.handle(COMMITS, ViewEvent::Activate(key.into()), &host.invocation()).unwrap();
    }

    fn commit_requests(host: &MockHost) -> Vec<String> {
        host.requests().into_iter().map(|r| r.url).filter(|u| u.contains("/repository/commits/")).collect()
    }

    #[test]
    fn lists_the_commits_of_the_selected_merge_request() {
        let host = host();
        let mut plugin = selected(&host);

        let view = plugin.render(COMMITS, None, &host.invocation()).unwrap();

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
        assert!(host.refreshed_views().contains(&DETAILS.to_owned()));
        let html = changes_html(&mut plugin, &host);
        assert!(html.contains("src/b.rs") && !html.contains("src/a.rs"), "{html}");
        assert!(!html.contains("Why?") && !html.contains("data-reply"), "no comment threads in commit mode");
        assert!(html.contains("aaaaaaa1"), "{html}");
        assert_eq!(commit_requests(&host), [format!("{REPO_URL}/commits/{SHA1}/diff?per_page=100&page=1")]);

        let comment = r#"{"type":"comment","oldPath":"src/b.rs","newPath":"src/b.rs","old":null,"new":2,"body":"x","draft":false}"#;
        changes_message(&mut plugin, &host, ViewEvent::Message(comment.into())).unwrap();
        assert!(writes(&host).is_empty(), "commit diffs are read-only");
    }

    #[test]
    fn all_changes_returns_to_the_merge_request_diff() {
        let host = host();
        let mut plugin = on_mr(&host);
        pick(&mut plugin, &host, SHA1);
        let view = plugin.render(COMMITS, None, &host.invocation()).unwrap();
        assert!(buttons(&view).contains(&"all-changes"), "{view:?}");

        plugin.handle(COMMITS, ViewEvent::Button("all-changes".into()), &host.invocation()).unwrap();

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
        assert!(host.refreshed_views().contains(&COMMITS.to_owned()));
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
            plugin.handle(COMMITS, ViewEvent::Select(key.into()), &host.invocation()).unwrap();
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

        let view = plugin.render(COMMITS, None, &host.invocation()).unwrap();

        assert_eq!(view.body, Body::Message("!7 has no commits.".into()));
    }

    #[test]
    fn the_view_is_quiet_without_a_selected_merge_request() {
        let host = host();
        let mut plugin = GitLab::default();

        let view = plugin.render(COMMITS, None, &host.invocation()).unwrap();
        plugin.handle(COMMITS, ViewEvent::Activate(SHA1.into()), &host.invocation()).unwrap();

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

        let view = plugin.render(COMMITS, None, &host.invocation()).unwrap();
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
        let view = plugin.render(COMMITS, None, &host.invocation()).unwrap();
        assert!(view.status.as_deref().unwrap_or("").contains("aaaaaaa1"), "{view:?}");
    }

    #[test]
    fn crlf_and_multibyte_diff_lines_keep_their_numbers() {
        let lines = review::parse_diff("@@ -1,2 +1,2 @@\r\n é\r\n-ü😀\r\n+ü😀x\r\n z\r\n");

        let numbers: Vec<(char, Option<u32>, Option<u32>, &str)> = lines.iter().map(|l| (l.kind, l.old, l.new, l.text.as_str())).collect();
        assert_eq!(
            numbers,
            [('@', None, None, "@@ -1,2 +1,2 @@"), (' ', Some(1), Some(1), "é"), ('-', Some(2), None, "ü😀"), ('+', None, Some(2), "ü😀x"), (' ', Some(3), Some(3), "z")]
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
            ("GET", "/merge_requests/7/pipelines") => ok(
                r#"[{"id": 41, "status": "running", "web_url": "https://gitlab.com/g/p/-/pipelines/41"},
                    {"id": 40, "status": "failed", "web_url": "https://gitlab.com/g/p/-/pipelines/40"}]"#,
            ),
            ("GET", "/pipelines/41/jobs") => ok(JOBS_41),
            ("GET", "/pipelines/40/jobs") => ok(r#"[{"id": 300, "name": "unit", "stage": "test", "status": "failed", "web_url": "https://gitlab.com/g/p/-/jobs/300"}]"#),
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
        plugin.render(PIPELINES, None, &host.invocation()).unwrap();
        plugin
    }

    fn pick(plugin: &mut GitLab, host: &MockHost, key: &str) {
        plugin.handle(PIPELINES, ViewEvent::Select(key.into()), &host.invocation()).unwrap();
    }

    fn click(plugin: &mut GitLab, host: &MockHost, id: &str) -> View {
        plugin.handle(PIPELINES, ViewEvent::Button(id.into()), &host.invocation()).unwrap();
        plugin.render(PIPELINES, None, &host.invocation()).unwrap()
    }

    fn rows(view: &View) -> Vec<(&str, Option<&str>, &str, Option<&str>)> {
        let Body::Tree(items) = &view.body else { panic!("{view:?}") };

        items.iter().map(|i| (i.key.as_str(), i.parent.as_deref(), i.label.as_str(), i.icon.as_deref())).collect()
    }

    #[test]
    fn shows_pipelines_with_jobs_grouped_by_stage_and_status_icons() {
        let host = host();
        let mut plugin = selected(&host);

        let view = plugin.render(PIPELINES, None, &host.invocation()).unwrap();

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

        plugin.handle(PIPELINES, ViewEvent::Expand("p:40".into()), &host.invocation()).unwrap();
        let view = plugin.render(PIPELINES, None, &host.invocation()).unwrap();

        assert!(rows(&view).contains(&("j:300", Some("s:40:test"), "unit", Some("error"))), "{view:?}");
        assert!(host.refreshed_views().contains(&PIPELINES.to_owned()));
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
            plugin.handle(PIPELINES, ViewEvent::Expand(key.into()), &host.invocation()).unwrap();
            for button in ["retry", "cancel", "cancel", "browser"] {
                plugin.handle(PIPELINES, ViewEvent::Button(button.into()), &host.invocation()).unwrap();
            }
        }

        assert_eq!(host.requests().len(), before);
        assert!(host.opened_urls().is_empty());
        plugin.render(PIPELINES, None, &host.invocation()).unwrap();
        assert!(host.requests()[before..].iter().all(|r| !r.url.contains("..") && !r.url.contains("?x") && !r.url.contains("/-1")));
    }

    #[test]
    fn open_in_browser_refuses_a_job_url_on_another_host() {
        let host = setup();
        host.on_fetch(|req| match req.url.contains("/pipelines/41/jobs") {
            true => ok(
                r#"[{"id": 1, "name": "evil", "stage": "test", "status": "failed", "web_url": "https://evil.test/j/1"},
                    {"id": 2, "name": "script", "stage": "test", "status": "failed", "web_url": "javascript:alert(1)"}]"#,
            ),
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
    fn retry_is_disabled_for_a_running_job() {
        let host = host();
        let mut plugin = on_mr(&host);
        pick(&mut plugin, &host, "j:302");

        let view = click(&mut plugin, &host, "retry");

        let retry = view.toolbar.iter().find(|b| b.id == "retry").unwrap();
        assert_eq!(retry, &Button::new("retry", "Retry Job").icon("rerun").enabled(false));
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

        let view = plugin.render(PIPELINES, None, &host.invocation()).unwrap();

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

        let view = plugin.render(PIPELINES, None, &host.invocation()).unwrap();
        pick(&mut plugin, &host, "j:301");
        plugin.handle(PIPELINES, ViewEvent::Expand("p:40".into()), &host.invocation()).unwrap();
        for button in ["retry", "cancel", "cancel", "browser"] {
            plugin.handle(PIPELINES, ViewEvent::Button(button.into()), &host.invocation()).unwrap();
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
    host.answer_next_prompt(Some(&["My files"]));

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

    assert_eq!(host.revealed_views(), ["merge-request"]);
}

#[test]
fn create_merge_request_reveals_the_form() {
    let host = setup();
    host.set_vcs_status(Some(status("feature")));

    GitLab::default().run_command("create-merge-request", &host.invocation(), None).unwrap();

    assert_eq!(host.revealed_views(), ["new-merge-request"]);
    assert!(host.opened_urls().is_empty());
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

    assert_eq!(
        host.opened_urls(),
        ["https://gitlab.com/g/p/-/blob/abc123/src/a%20b.rs", "https://gitlab.com/g/p/-/tree/abc123/src"]
    );
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
fn section_buttons_list_the_five_sections_with_the_active_one_marked() {
    let host = setup();
    let mut plugin = selected(&host);

    let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();

    let ids: Vec<&str> = section_buttons(&view).into_iter().map(|(id, _)| id).collect();
    assert_eq!(ids, ["section:overview", "section:changes", "section:commits", "section:files", "section:pipelines"]);
    assert_eq!(section_buttons(&view)[0].1, "● Overview");
    assert_eq!(section_buttons(&view)[1].1, "Changes");

    let view = changes_view(&mut plugin, &host).unwrap();

    assert_eq!(section_buttons(&view)[0].1, "Overview");
    assert_eq!(section_buttons(&view)[1].1, "● Changes (1)");
}

#[test]
fn clicking_a_section_button_switches_the_view_and_stores_it() {
    let host = setup();
    let mut plugin = selected(&host);

    plugin.handle(DETAILS, ViewEvent::Button("section:changes".into()), &host.invocation()).unwrap();

    assert_eq!(storage::get(sections::SECTION_KEY).unwrap().as_deref(), Some("changes"));
    assert!(host.refreshed_views().contains(&DETAILS.to_owned()));
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
        assert_eq!(section_buttons(&view).len(), 5);
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

    plugin.handle(DETAILS, ViewEvent::Message(r#"{"type":"viewed","path":"src/a.rs","viewed":true}"#.into()), &host.invocation()).unwrap();

    let view = plugin.render(DETAILS, None, &host.invocation()).unwrap();
    let Body::Html(html) = &view.body else { panic!("{view:?}") };
    assert!(html.contains("src/a.rs"));
}
