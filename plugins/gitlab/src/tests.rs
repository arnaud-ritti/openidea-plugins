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
    "diff_refs": {"base_sha": "base", "head_sha": "head", "start_sha": "start"},
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
        ("GET", p) if p == mr => ok(MR),
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
        r#"{"body":"gone?","position":{"base_sha":"base","head_sha":"head","new_line":null,"new_path":"src/a.rs","old_line":4,"old_path":"src/a.rs","position_type":"text","start_sha":"start"}}"#
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

    assert_eq!(host.storage().get("viewed:g/p:7").map(String::as_str), Some(r#"{"src/a.rs":"head"}"#));
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
    assert_eq!(terminal.args, ["clone", "https://gitlab.com/me/tool.git", "/tool"]);

    host.set_setting("cloneDirectory", r#""/src""#);
    host.set_setting("cloneUsingSsh", "true");
    plugin.handle(PROJECTS, ViewEvent::Activate("me/tool".into()), &host.invocation()).unwrap();
    assert_eq!(host.terminals()[1].args, ["clone", "git@gitlab.com:me/tool.git", "/src/tool"]);
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
    assert_eq!(git_runs(&host), ["--version", "remote add gitlab https://gitlab.com/me/project.git"]);
    assert_eq!(host.terminals()[0].args, ["push", "-u", "gitlab", "main"]);
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
    let stdout = match args[0].as_str() {
        "log" => "\u{1}c2\n\nsrc/b.rs\n\u{1}c1\n\nsrc/a.rs\nsrc/b.rs\n".to_owned(),
        _ => String::new(),
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

    assert_eq!(storage::get("viewed:g/p:7").unwrap().unwrap(), r#"{"src/a.rs":"head"}"#);
    assert!(changes_html(&mut plugin, &host).contains(" checked"));
}
