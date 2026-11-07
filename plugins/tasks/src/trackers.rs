//! Issue trackers, after the `TaskRepository` implementations of
//! intellij-community (GithubRepository, GitlabRepository, ForgejoRepository,
//! YouTrackRepository, JiraRestApiCloud3, TrelloRepository). Each is used
//! once the user has entered its token.

use std::collections::BTreeMap;

use openidea_plugin::{Error, HttpRequest, host};
use serde_json::{Value, json};

use crate::model::Task;

const LIMIT: u32 = 50;

type Remotes = [(String, String)];

/// What a fetch looks at: `query` turns the listing into a search, `repositories` (by tracker id) override the settings.
pub struct Scope<'a> {
    pub remotes: &'a Remotes,
    pub query: Option<&'a str>,
    pub repositories: &'a BTreeMap<String, Vec<String>>,
}

pub struct Tracker {
    pub id: &'static str,
    pub name: &'static str,
    secrets: &'static [&'static str],
    headers: &'static [(&'static str, &'static str)],
    fetch: fn(&Tracker, &Scope) -> Result<Vec<Task>, Error>,
}

const TRACKERS: [Tracker; 6] = [
    Tracker {
        id: "github",
        name: "GitHub",
        secrets: &["github-token"],
        headers: &[
            ("Authorization", "Bearer {{secret:github-token}}"),
            ("Accept", "application/vnd.github+json"),
            ("User-Agent", "openidea-tasks"),
        ],
        fetch: github,
    },
    Tracker {
        id: "gitlab",
        name: "GitLab",
        secrets: &["gitlab-token"],
        headers: &[("PRIVATE-TOKEN", "{{secret:gitlab-token}}")],
        fetch: gitlab,
    },
    Tracker {
        id: "forgejo",
        name: "Codeberg",
        secrets: &["forgejo-token"],
        headers: &[("Authorization", "Bearer {{secret:forgejo-token}}"), ("Accept", "application/json")],
        fetch: forgejo,
    },
    Tracker {
        id: "youtrack",
        name: "YouTrack",
        secrets: &["youtrack-token"],
        headers: &[("Authorization", "Bearer {{secret:youtrack-token}}"), ("Accept", "application/json")],
        fetch: youtrack,
    },
    Tracker {
        id: "jira",
        name: "Jira",
        secrets: &["jira-token"],
        headers: &[("Authorization", "Basic {{secret:jira-token}}"), ("Accept", "application/json")],
        fetch: jira,
    },
    Tracker {
        id: "trello",
        name: "Trello",
        secrets: &["trello-key", "trello-token"],
        headers: &[(
            "Authorization",
            "OAuth oauth_consumer_key=\"{{secret:trello-key}}\", oauth_token=\"{{secret:trello-token}}\"",
        )],
        fetch: trello,
    },
];

#[derive(Debug, Default)]
pub struct Fetched {
    pub issues: Vec<Task>,
    pub errors: Vec<String>,
    pub trackers: usize,
}

/// Open issues (or the search hits) of every configured tracker; one failing doesn't stop the others.
pub fn fetch_all(scope: &Scope) -> Fetched {
    let mut fetched = Fetched::default();

    for tracker in &TRACKERS {
        if !tracker.secrets.iter().all(|s| host::secret_is_set(s)) {
            continue;
        }

        fetched.trackers += 1;

        match (tracker.fetch)(tracker, scope) {
            Ok(issues) => fetched.issues.extend(issues),
            Err(e) => fetched.errors.push(format!("{}: {e}", tracker.name)),
        }
    }

    fetched
}

pub fn is_repository_tracker(id: &str) -> bool {
    matches!(id, "github" | "gitlab" | "forgejo")
}

pub fn tracker_name(id: &str) -> &str {
    TRACKERS.iter().find(|t| t.id == id).map_or(id, |t| t.name)
}

pub fn name(task: &Task) -> &str {
    let id = task.tracker.as_deref().unwrap_or_default();

    TRACKERS.iter().find(|t| t.id == id).map_or(id, |t| t.name)
}

/// The Close Task form offers "Update issue state" for these.
pub fn can_update_state(task: &Task) -> bool {
    matches!(task.tracker.as_deref(), Some("github" | "gitlab" | "forgejo" | "youtrack" | "jira"))
}

pub fn can_post_time(task: &Task) -> bool {
    matches!(task.tracker.as_deref(), Some("youtrack" | "jira"))
}

/// TaskRepository.setTaskState with the tracker's resolved state.
pub fn close_issue(task: &Task) -> Result<(), Error> {
    let (tracker, api) = issue_api(task)?;

    match tracker.id {
        "github" | "forgejo" => send(tracker, "PATCH", api, Some(json!({ "state": "closed" }))).map(drop),
        "gitlab" => send(tracker, "PUT", &format!("{api}?state_event=close"), None).map(drop),
        "youtrack" => {
            let state = setting("youtrackCloseState").unwrap_or_else(|| "Fixed".into());
            let commands = format!("{}/api/commands", origin(api));
            let command = json!({ "query": format!("state {state}"), "issues": [{ "idReadable": task.id }] });

            send(tracker, "POST", &commands, Some(command)).map(drop)
        }
        "jira" => {
            let transitions = send(tracker, "GET", &format!("{api}/transitions"), None)?;
            let done = items(&transitions["transitions"], |t| {
                (t["to"]["statusCategory"]["key"] == "done").then(|| t["id"].as_str().map(str::to_owned)).flatten()
            });
            let id = done.first().ok_or("None of this issue's transitions leads to a done status.")?;

            send(tracker, "POST", &format!("{api}/transitions"), Some(json!({ "transition": { "id": id } }))).map(drop)
        }
        _ => Err(format!("{} issues can't be closed from the IDE.", tracker.name).into()),
    }
}

/// YouTrackRepository.updateTimeSpent / JiraRestApi.updateTimeSpend.
pub fn post_time(task: &Task, seconds: u64) -> Result<(), Error> {
    let (tracker, api) = issue_api(task)?;

    match tracker.id {
        "youtrack" => {
            let item = json!({ "usesMarkdown": true, "text": "", "duration": { "minutes": seconds.div_ceil(60) } });
            send(tracker, "POST", &format!("{api}/timeTracking/workItems"), Some(item)).map(drop)
        }
        "jira" => send(tracker, "POST", &format!("{api}/worklog"), Some(json!({ "timeSpentSeconds": seconds.max(60) }))).map(drop),
        _ => Err(format!("Posting work time to {} isn't supported.", tracker.name).into()),
    }
}

fn issue_api(task: &Task) -> Result<(&'static Tracker, &str), Error> {
    let tracker = TRACKERS.iter().find(|t| Some(t.id) == task.tracker.as_deref()).ok_or("The task isn't a tracker issue.")?;
    let api = task.api.as_deref().ok_or("The issue has no API address. Update issues and try again.")?;

    Ok((tracker, api))
}

/// The repositories added in the Tasks view, else the setting, else the remote's.
fn repositories(scope: &Scope, tracker: &str, setting_key: &str, remote_host: &str) -> Vec<String> {
    let added = scope.repositories.get(tracker).cloned().unwrap_or_default();

    if !added.is_empty() {
        return added;
    }

    setting(setting_key).or_else(|| remote_path(scope.remotes, remote_host)).into_iter().collect()
}

fn github(tracker: &Tracker, scope: &Scope) -> Result<Vec<Task>, Error> {
    let repos = repositories(scope, "github", "githubRepository", "github.com");

    if repos.is_empty() {
        return Err("No github.com remote. Set the repository in Settings › Plugins › Tasks.".into());
    }

    let mut query = format!("{} is:issue is:open", repos.iter().map(|r| format!("repo:{r}")).collect::<Vec<_>>().join(" "));

    if host::setting_bool("githubAssignedOnly") == Some(true) {
        query.push_str(" assignee:@me");
    }

    if let Some(extra) = setting("githubQuery") {
        query = format!("{query} {extra}");
    }

    if let Some(text) = scope.query {
        query = format!("{query} {text}");
    }

    let url = format!("https://api.github.com/search/issues?q={}&sort=updated&per_page={LIMIT}", encode(&query));
    let first_name = repos[0].rsplit('/').next().unwrap_or(&repos[0]).to_owned();

    Ok(items(&send(tracker, "GET", &url, None)?["items"], |issue| {
        let repo_name = issue["repository_url"].as_str().and_then(|u| u.rsplit('/').next()).unwrap_or(&first_name);

        Some(Task::issue(
            "github",
            &format!("{repo_name}-{}", issue["number"].as_u64()?),
            issue["title"].as_str()?,
            issue["html_url"].as_str()?,
            issue["url"].as_str()?,
        ))
    }))
}

fn gitlab(tracker: &Tracker, scope: &Scope) -> Result<Vec<Task>, Error> {
    let projects = repositories(scope, "gitlab", "gitlabProject", "gitlab.com");
    let search = scope.query.map(|q| format!("&search={}", encode(q))).unwrap_or_default();
    let bases = match projects.is_empty() {
        true => vec!["https://gitlab.com/api/v4/issues".to_owned()],
        false => projects.iter().map(|p| format!("https://gitlab.com/api/v4/projects/{}/issues", encode(p))).collect(),
    };
    let single = projects.len() == 1;
    let mut found = Vec::new();

    for base in bases {
        let url = format!("{base}?state=opened&order_by=updated_at&per_page={LIMIT}{search}");

        found.extend(items(&send(tracker, "GET", &url, None)?, |issue| {
            let id = match single {
                true => format!("#{}", issue["iid"].as_u64()?),
                false => issue["references"]["full"].as_str()?.to_owned(),
            };

            Some(Task::issue("gitlab", &id, issue["title"].as_str()?, issue["web_url"].as_str()?, issue["_links"]["self"].as_str()?))
        }));
    }

    Ok(found)
}

fn forgejo(tracker: &Tracker, scope: &Scope) -> Result<Vec<Task>, Error> {
    let repos = repositories(scope, "forgejo", "forgejoRepository", "codeberg.org");

    if repos.is_empty() {
        return Err("No codeberg.org remote. Set the repository in Settings › Plugins › Tasks.".into());
    }

    let search = scope.query.map(|q| format!("&q={}", encode(q))).unwrap_or_default();
    let mut found = Vec::new();

    for repo in &repos {
        let url = format!("https://codeberg.org/api/v1/repos/{repo}/issues?type=issues&state=open&sort=updated&limit={LIMIT}{search}");
        let prefix = if repos.len() > 1 { repo.as_str() } else { "" };

        found.extend(items(&send(tracker, "GET", &url, None)?, |issue| {
            Some(Task::issue(
                "forgejo",
                &format!("{prefix}#{}", issue["number"].as_u64()?),
                issue["title"].as_str()?,
                issue["html_url"].as_str()?,
                issue["url"].as_str()?,
            ))
        }));
    }

    Ok(found)
}

fn youtrack(tracker: &Tracker, scope: &Scope) -> Result<Vec<Task>, Error> {
    let site = site("youtrackHost", "YouTrack")?;
    let query = scope.query.map(str::to_owned).or_else(|| setting("youtrackQuery")).unwrap_or_default();
    let url = format!(
        "https://{site}/api/issues?query={}&fields=idReadable,summary&$top={LIMIT}",
        encode(&query)
    );

    Ok(items(&send(tracker, "GET", &url, None)?, |issue| {
        let id = issue["idReadable"].as_str()?;

        Some(Task::issue(
            "youtrack",
            id,
            issue["summary"].as_str()?,
            &format!("https://{site}/issue/{id}"),
            &format!("https://{site}/api/issues/{id}"),
        ))
    }))
}

fn jira(tracker: &Tracker, scope: &Scope) -> Result<Vec<Task>, Error> {
    let site = site("jiraHost", "Jira")?;
    let jql = match scope.query {
        Some(text) => format!("text ~ \"{}\" order by updated DESC", text.replace('\\', "\\\\").replace('"', "\\\"")),
        None => setting("jiraQuery").unwrap_or_default(),
    };
    let url = format!(
        "https://{site}/rest/api/3/search/jql?jql={}&maxResults={LIMIT}&fields=summary",
        encode(&jql)
    );

    Ok(items(&send(tracker, "GET", &url, None)?["issues"], |issue| {
        let key = issue["key"].as_str()?;

        Some(Task::issue(
            "jira",
            key,
            issue["fields"]["summary"].as_str()?,
            &format!("https://{site}/browse/{key}"),
            &format!("https://{site}/rest/api/3/issue/{key}"),
        ))
    }))
}

/// The cards the user is a member of (TrelloRepository without a board), or the cards matching a search.
fn trello(tracker: &Tracker, scope: &Scope) -> Result<Vec<Task>, Error> {
    let (url, cards_at) = match scope.query {
        Some(text) => (
            format!("https://api.trello.com/1/search?query={}&modelTypes=cards&cards_limit={LIMIT}&card_fields=name,url", encode(text)),
            "cards",
        ),
        None => ("https://api.trello.com/1/members/me/cards?filter=visible&fields=name,url".to_owned(), ""),
    };
    let response = send(tracker, "GET", &url, None)?;
    let cards = if cards_at.is_empty() { &response } else { &response[cards_at] };

    Ok(items(cards, |card| {
        let id = card["id"].as_str()?;

        Some(Task::issue(
            "trello",
            id,
            card["name"].as_str()?,
            card["url"].as_str()?,
            &format!("https://api.trello.com/1/cards/{id}"),
        ))
    }))
}

fn send(tracker: &Tracker, method: &str, url: &str, body: Option<Value>) -> Result<Value, Error> {
    let mut request = HttpRequest { method: method.into(), url: url.into(), ..HttpRequest::default() };

    for (name, value) in tracker.headers {
        request = request.header(name, value);
    }

    if let Some(body) = body {
        request = request.header("Content-Type", "application/json");
        request.body = Some(body.to_string().into_bytes());
    }

    let response = host::fetch(&request)?;

    if response.status >= 400 {
        return Err(format!("HTTP {}: {}", response.status, response.text().chars().take(200).collect::<String>()).into());
    }

    if response.body.is_empty() {
        return Ok(Value::Null);
    }

    serde_json::from_slice(&response.body).map_err(|e| format!("Unexpected response: {e}.").into())
}

fn items<T>(list: &Value, item: impl Fn(&Value) -> Option<T>) -> Vec<T> {
    list.as_array().into_iter().flatten().filter_map(item).collect()
}

fn setting(key: &str) -> Option<String> {
    host::setting_string(key).map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

/// A host setting, without the `https://` or path users paste along.
fn site(key: &str, tracker: &str) -> Result<String, Error> {
    let value = setting(key).ok_or_else(|| format!("Set the {tracker} host in Settings › Plugins › Tasks."))?;
    let value = value.split_once("://").map_or(value.as_str(), |(_, rest)| rest);

    Ok(value.split('/').next().unwrap_or_default().to_ascii_lowercase())
}

/// `https://host` of an API address.
fn origin(url: &str) -> &str {
    let host_end = url.find("://").map_or(0, |i| i + 3);

    url[host_end..].find('/').map_or(url, |i| &url[..host_end + i])
}

/// `owner/name` of the first remote on `host`, `origin` first.
fn remote_path(remotes: &Remotes, host: &str) -> Option<String> {
    let mut found: Vec<_> = remotes.iter().filter_map(|(name, url)| Some((name != "origin", path_on(url, host)?))).collect();
    found.sort();

    found.into_iter().next().map(|(_, path)| path)
}

fn path_on(url: &str, host: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.rsplit_once('@').map_or(rest, |(_, rest)| rest);
    let path = rest.strip_prefix(host)?.strip_prefix([':', '/'])?;
    let path = path.trim_end_matches('/').trim_end_matches(".git");

    path.contains('/').then(|| path.to_owned())
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::HttpResponse;
    use openidea_plugin::mock::MockHost;

    fn respond(host: &MockHost, body: &'static str) {
        host.on_fetch(move |_| Ok(HttpResponse { status: 200, headers: vec![], body: body.into() }));
    }

    #[test]
    fn finds_the_project_of_ssh_and_https_remotes() {
        let remotes = vec![
            ("fork".to_owned(), "https://github.com/me/tool.git".to_owned()),
            ("origin".to_owned(), "git@github.com:acme/tool.git".to_owned()),
            ("lab".to_owned(), "ssh://git@gitlab.com/group/sub/proj".to_owned()),
        ];

        assert_eq!(remote_path(&remotes, "github.com").as_deref(), Some("acme/tool"));
        assert_eq!(remote_path(&remotes, "gitlab.com").as_deref(), Some("group/sub/proj"));
        assert_eq!(remote_path(&remotes, "bitbucket.org"), None);
    }

    #[test]
    fn encodes_query_values() {
        assert_eq!(encode("repo:a/b is:open"), "repo%3Aa%2Fb%20is%3Aopen");
        assert_eq!(origin("https://acme.youtrack.cloud/api/issues/X-1"), "https://acme.youtrack.cloud");
    }

    #[test]
    fn closes_a_jira_issue_through_its_done_transition() {
        let host = MockHost::install().with_capabilities(&["secrets", "net:acme.atlassian.net"]);
        host.set_secret("jira-token", "Zm9v");
        respond(&host, r#"{"transitions":[{"id":"11","to":{"statusCategory":{"key":"indeterminate"}}},{"id":"31","to":{"statusCategory":{"key":"done"}}}]}"#);
        let task = Task::issue("jira", "PROJ-7", "Crash", "https://acme.atlassian.net/browse/PROJ-7", "https://acme.atlassian.net/rest/api/3/issue/PROJ-7");

        close_issue(&task).unwrap();

        let requests = host.requests();
        assert_eq!(requests[1].method, "POST");
        assert_eq!(requests[1].url, "https://acme.atlassian.net/rest/api/3/issue/PROJ-7/transitions");
        assert_eq!(requests[1].body.as_deref(), Some(br#"{"transition":{"id":"31"}}"#.as_slice()));
        assert!(requests[1].headers.contains(&("Authorization".into(), "Basic Zm9v".into())));
    }

    #[test]
    fn posts_youtrack_work_items_in_minutes() {
        let host = MockHost::install().with_capabilities(&["secrets", "net:acme.youtrack.cloud"]);
        host.set_secret("youtrack-token", "perm:x");
        respond(&host, "");
        let task = Task::issue("youtrack", "X-1", "Crash", "https://acme.youtrack.cloud/issue/X-1", "https://acme.youtrack.cloud/api/issues/X-1");

        post_time(&task, 3601).unwrap();

        let request = &host.requests()[0];
        assert_eq!(request.url, "https://acme.youtrack.cloud/api/issues/X-1/timeTracking/workItems");
        let body: Value = serde_json::from_slice(request.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["duration"]["minutes"], 61);
    }

    #[test]
    fn reads_trello_cards_with_both_secrets_in_one_header() {
        let host = MockHost::install().with_capabilities(&["secrets", "net:api.trello.com"]);
        host.set_secret("trello-key", "k");
        host.set_secret("trello-token", "t");
        respond(&host, r#"[{"id":"5f3a","name":"Ship it","url":"https://trello.com/c/abc"}]"#);

        let fetched = fetch_all(&Scope { remotes: &[], query: None, repositories: &BTreeMap::new() });

        assert_eq!(fetched.trackers, 1);
        assert_eq!(fetched.issues[0].label(), "5f3a: Ship it");
        let auth = &host.requests()[0].headers[0].1;
        assert_eq!(auth, r#"OAuth oauth_consumer_key="k", oauth_token="t""#);
    }
}
