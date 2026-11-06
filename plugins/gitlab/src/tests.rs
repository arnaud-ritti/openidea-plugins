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
        ("GET", "/api/v4/users?username=ann") => ok(r#"[{"id": 2, "username": "ann"}]"#),
        ("GET", "/api/v4/users?username=bob") => ok(r#"[{"id": 3, "username": "bob"}]"#),
        ("GET", p) if p.starts_with("/api/v4/users?") => ok("[]"),
        ("POST", "/api/v4/projects/g%2Fp/merge_requests") => ok(r#"{"iid": 8, "title": "Add parser", "web_url": "https://gitlab.com/g/p/-/merge_requests/8"}"#),
        ("POST", "/api/v4/snippets") => ok(r#"{"web_url": "https://gitlab.com/-/snippets/9"}"#),
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

    assert_eq!(
        buttons(&view),
        ["comment", "approve", "merge", "review", "draft", "close", "checkout", "browser", "copy", "refresh"]
    );
    assert!(view.toolbar.iter().all(|b| b.icon.is_some()), "{view:?}");
    assert_eq!(
        view.status.as_deref(),
        Some("Open · feature → main · ann · Ready to merge · Pipeline passed · Approved by bob · 1 more required")
    );
    let Body::Form(fields) = &view.body else { panic!("{view:?}") };
    let keys: Vec<&str> = fields.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["comment", "as-draft", "remove-source-branch", "squash", "description"]);
    assert_eq!(fields[4].label, "Description (read-only)");
    assert_eq!(fields[4].value, FieldValue::Multiline("Adds it".into()));
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
    assert!(host.refreshed_views().ends_with(&[DETAILS.into(), CHANGES.into(), LIST.into()]));
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

    let view = plugin.render(CHANGES, None, &host.invocation()).unwrap();

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
    plugin.handle(CHANGES, ViewEvent::Message(r#"{"type":"viewed","path":"src/a.rs","viewed":true}"#.into()), &host.invocation()).unwrap();

    let view = plugin.render(CHANGES, None, &host.invocation()).unwrap();

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
    plugin.render(CHANGES, None, &host.invocation()).unwrap();

    let message = r#"{"type":"comment","oldPath":"src/a.rs","newPath":"src/a.rs","old":4,"new":null,"body":"gone?","draft":false,"anchor":"f0-4-"}"#;
    plugin.handle(CHANGES, ViewEvent::Message(message.into()), &host.invocation()).unwrap();
    let draft = r#"{"type":"comment","oldPath":"src/a.rs","newPath":"src/a.rs","old":3,"new":3,"body":"ctx","draft":true}"#;
    plugin.handle(CHANGES, ViewEvent::Message(draft.into()), &host.invocation()).unwrap();

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
    plugin.render(CHANGES, None, &host.invocation()).unwrap();
    let refreshed = host.refreshed_views().len();

    let message = r#"{"type":"comment","oldPath":"src/a.rs","newPath":"src/a.rs","old":4,"new":null,"body":"gone?","draft":false,"anchor":"f0-4-"}"#;
    plugin.handle(CHANGES, ViewEvent::Message(message.into()), &host.invocation()).unwrap();

    assert_eq!(host.refreshed_views().len(), refreshed);
    assert_eq!(plugin.status, None);
    assert_eq!(plugin.anchor, None);
    let (view, posted) = &host.view_messages()[0];
    assert_eq!(view, CHANGES);
    assert!(posted.contains(r#""type":"error""#) && posted.contains("line_code is invalid"), "{posted}");
}

#[test]
fn viewed_files_are_remembered_for_the_head_commit() {
    let host = setup();
    let mut plugin = selected(&host);
    plugin.render(CHANGES, None, &host.invocation()).unwrap();

    plugin.handle(CHANGES, ViewEvent::Message(r#"{"type":"viewed","path":"src/a.rs","viewed":true}"#.into()), &host.invocation()).unwrap();
    let view = plugin.render(CHANGES, None, &host.invocation()).unwrap();

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
            "fetch origin +refs/heads/feature:refs/remotes/origin/feature",
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
fn the_token_never_appears_in_requests_logs_notifications_or_views() {
    let host = setup();
    host.on_process("git", git_ok);
    let mut plugin = GitLab::default();
    let mut views = Vec::new();

    views.push(format!("{:?}", plugin.render(LIST, None, &host.invocation()).unwrap()));
    plugin.handle(LIST, ViewEvent::Select("mr:7".into()), &host.invocation()).unwrap();
    views.push(format!("{:?}", plugin.render(DETAILS, None, &host.invocation()).unwrap()));
    views.push(format!("{:?}", plugin.render(CHANGES, None, &host.invocation()).unwrap()));
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
    let view = plugin.render(CHANGES, None, &host.invocation()).unwrap();
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

    plugin.handle(CHANGES, ViewEvent::Message(r#"{"type":"viewed","path":"src/a.rs","viewed":true}"#.into()), &host.invocation()).unwrap();
    assert_eq!(storage::get("viewed:g/p:7").unwrap().unwrap(), r#"{"src/a.rs":"c1"}"#);
}

#[test]
fn viewed_falls_back_to_the_head_without_local_history() {
    let host = setup();
    host.on_process("git", |_, _| Ok(ProcessOutput { status: Some(128), stdout: String::new(), stderr: "bad object".into() }));
    let mut plugin = selected(&host);

    plugin.handle(CHANGES, ViewEvent::Message(r#"{"type":"viewed","path":"src/a.rs","viewed":true}"#.into()), &host.invocation()).unwrap();

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

    plugin.handle(CHANGES, ViewEvent::Message(r#"{"type":"viewed","path":"src/a.rs","viewed":true}"#.into()), &host.invocation()).unwrap();

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
        let yaml = "a:\n  needs:\n    - project: g/p\n      job: other\n      ref: main\n    - pipeline: $PARENT\n      job: x\n    - $DYNAMIC\n    - job: \"${NAME}\"\n    - *alias\n  extends: !reference [.x]\n  script: [x]\n";

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
        assert!(codes("stages: !reference [.s]\njob:\n  stage: a\n").is_empty());
    }

    #[test]
    fn flow_mapping_job_counts_as_defined() {
        assert!(codes("build: {script: [make]}\ntest:\n  needs: [build]\n  extends: build\n").is_empty());
    }

    #[test]
    fn tagged_job_counts_as_defined() {
        assert!(codes("build: !reference [.x, y]\ntest:\n  needs: [build]\n").is_empty());
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
        assert_eq!(plan.fetch, ["fetch", "origin", "+refs/heads/feature/x:refs/remotes/origin/feature/x"]);
        assert_eq!(plan.add, ["worktree", "add", "--detach", "--", "/work/tool-feature-x", "origin/feature/x"]);
    }

    #[test]
    fn fork_worktree_checks_out_the_fetched_merge_request_head() {
        let mut mr = merge_request("fix");
        mr.source_project_id = 9;
        mr.author.username = "ann".into();
        let plan = branch::worktree_plan("/work/tool", "origin", &mr).unwrap();

        assert_eq!(plan.fetch, ["fetch", "origin", "+refs/merge-requests/7/head:refs/remotes/origin/fork/ann/fix"]);
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
                "fetch origin +refs/heads/feature:refs/remotes/origin/feature",
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
