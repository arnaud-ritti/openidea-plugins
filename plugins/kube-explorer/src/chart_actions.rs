//! Quick fixes in `Chart.yaml`: a dependency stanza with placeholders, and a
//! dependency's repository as an alias or as a URL. The aliases come from a
//! `helm repo list` result that a command stored; a provider never runs helm
//! (it has 50 ms) and cannot ask the user anything.

use openidea_plugin::providers::{CodeAction, Document};
use openidea_plugin::text::position;
use openidea_plugin::{Error, NotificationLevel, Position, Range, TextEdit, host};

use crate::{HELM, KubeExplorer};

pub const ADD_DEPENDENCY: &str = "Add chart dependency";
const CACHE_KEY: &str = "helm-repos";

fn cached_repos() -> Vec<(String, String)> {
    let text = host::storage::get(CACHE_KEY).ok().flatten().unwrap_or_default();
    text.lines().filter_map(|line| line.split_once('\t')).map(|(name, url)| (name.to_owned(), url.to_owned())).collect()
}

fn string_after(object: &str, key: &str) -> Option<String> {
    let rest = &object[object.find(&format!("\"{key}\""))? + key.len() + 2..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_owned())
}

/// `helm repo list -o json`: `[{"name":"bitnami","url":"https://…"}]`.
fn parse_repos(json: &str) -> Vec<(String, String)> {
    json.split('{')
        .skip(1)
        .filter_map(|object| Some((string_after(object, "name")?, string_after(object, "url")?)))
        .filter(|(name, url)| !name.contains(['\t', '\n']) && !url.contains(['\t', '\n']))
        .collect()
}

impl KubeExplorer {
    pub fn cache_helm_repositories(&self) -> Result<(), Error> {
        let args = ["repo".to_owned(), "list".to_owned(), "-o".to_owned(), "json".to_owned()];
        let out = match host::run_process(HELM, &args, None) {
            Err(Error::Denied(_)) => {
                host::notify(
                    NotificationLevel::Info,
                    "Helm repositories",
                    "Allow the optional process:helm capability to read the repository list.",
                );
                return Ok(());
            }
            other => other?,
        };
        let repos = match out.status {
            Some(0) => parse_repos(&out.stdout),
            _ => Vec::new(),
        };
        let lines: Vec<String> = repos.iter().map(|(name, url)| format!("{name}\t{url}")).collect();
        match lines.is_empty() {
            true => host::storage::delete(CACHE_KEY)?,
            false => host::storage::set(CACHE_KEY, &lines.join("\n"))?,
        }
        crate::config::notify_success("Helm repositories", &format!("{} repositories are now offered as aliases in Chart.yaml.", repos.len()));
        Ok(())
    }
}

struct Lines<'a> {
    text: &'a str,
    lines: Vec<(usize, &'a str)>,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        let mut offset = 0;
        let lines = text
            .split_inclusive('\n')
            .map(|line| {
                let at = offset;
                offset += line.len();
                (at, line.trim_end_matches(['\r', '\n']))
            })
            .collect();
        Self { text, lines }
    }

    fn eol(&self) -> &'static str {
        match self.text.contains("\r\n") {
            true => "\r\n",
            false => "\n",
        }
    }

    /// Line indexes of the `dependencies:` block: the key's line, then its entries.
    fn dependencies(&self) -> Option<(usize, usize)> {
        let key = self.lines.iter().position(|(_, line)| line.starts_with("dependencies:"))?;
        if !self.lines[key].1["dependencies:".len()..].trim().is_empty() {
            return None;
        }
        let end = (key + 1..self.lines.len())
            .find(|i| {
                let line = self.lines[*i].1;
                !line.trim().is_empty() && !line.trim_start().starts_with('#') && !line.starts_with(' ') && !line.starts_with('-')
            })
            .unwrap_or(self.lines.len());
        Some((key, end))
    }
}

fn is_chart_v2(text: &str) -> bool {
    text.lines().any(|line| line.trim_end() == "apiVersion: v2" || line.trim_end() == "apiVersion: \"v2\"")
}

fn add_dependency(doc: &Document) -> Option<CodeAction> {
    if !is_chart_v2(&doc.text) || doc.text.contains("{{") {
        return None;
    }
    let lines = Lines::new(&doc.text);
    let eol = lines.eol();
    let dependencies = lines.dependencies();
    if dependencies.is_none() && lines.lines.iter().any(|(_, line)| line.starts_with("dependencies:")) {
        return None;
    }
    let (at, indent) = match dependencies {
        Some((key, end)) => {
            let last = (key..end).rev().find(|i| !lines.lines[*i].1.trim().is_empty())?;
            let first_item = (key + 1..end).find(|i| lines.lines[*i].1.trim_start().starts_with("- "));
            let indent = first_item.map_or(2, |i| lines.lines[i].1.len() - lines.lines[i].1.trim_start().len());
            (lines.lines[last].0 + lines.lines[last].1.len(), indent)
        }
        None => (doc.text.len(), 2),
    };
    let pad = " ".repeat(indent);
    let header = match dependencies {
        Some(_) => String::new(),
        None => format!("dependencies:{eol}"),
    };
    let at_end = at == doc.text.len();
    let prefix = match (at_end, doc.text.is_empty() || doc.text.ends_with('\n')) {
        (true, true) => "",
        _ => eol,
    };
    let stanza = format!("{prefix}{header}{pad}- name: NAME{eol}{pad}  version: \"VERSION\"{eol}{pad}  repository: \"REPOSITORY\"");
    let stanza = match at_end {
        true => format!("{stanza}{eol}"),
        false => stanza,
    };
    let point = position(&doc.text, at);
    Some(CodeAction {
        title: ADD_DEPENDENCY.into(),
        kind: Some("refactor.rewrite".into()),
        preferred: false,
        edits: vec![(doc.path.clone(), vec![TextEdit { range: Range::new(point, point), text: stanza }])],
        data: None,
    })
}

fn repository_value(line: &str) -> Option<(usize, usize, &str)> {
    let key_at = line.find("repository:")?;
    if !line[..key_at].trim().is_empty() && line[..key_at].trim() != "-" {
        return None;
    }
    let after = key_at + "repository:".len();
    let rest = &line[after..];
    let start = after + rest.len() - rest.trim_start().len();
    let value = line[start..].trim_end();
    let value = match value.split_once(" #") {
        Some((before, _)) => before.trim_end(),
        None => value,
    };
    (!value.is_empty()).then_some((start, start + value.len(), value.trim_matches(['"', '\''])))
}

fn repository_action(doc: &Document, caret: Position) -> Option<CodeAction> {
    let repos = cached_repos();
    if repos.is_empty() || !is_chart_v2(&doc.text) {
        return None;
    }
    let line = doc.text.lines().nth(caret.line as usize)?;
    let (start, end, value) = repository_value(line)?;
    let (title, replacement) = if let Some(name) = value.strip_prefix('@').or_else(|| value.strip_prefix("alias:")) {
        let (_, url) = repos.iter().find(|(known, _)| known == name)?;
        (format!("Use repository URL {url}"), format!("\"{url}\""))
    } else {
        let (name, _) = repos.iter().find(|(_, url)| url.trim_end_matches('/') == value.trim_end_matches('/'))?;
        (format!("Use repository alias @{name}"), format!("\"@{name}\""))
    };
    let column = |byte: usize| line[..byte].chars().map(char::len_utf16).sum::<usize>() as u32;
    let range = Range::new(Position::new(caret.line, column(start)), Position::new(caret.line, column(end)));
    Some(CodeAction {
        title,
        kind: Some("refactor.rewrite".into()),
        preferred: false,
        edits: vec![(doc.path.clone(), vec![TextEdit { range, text: replacement }])],
        data: None,
    })
}

pub fn code_actions(doc: &Document, selection: Range) -> Vec<CodeAction> {
    if doc.path.rsplit('/').next() != Some("Chart.yaml") || doc.text.len() > crate::MAX_BYTES {
        return Vec::new();
    }
    add_dependency(doc).into_iter().chain(repository_action(doc, selection.start)).collect()
}

#[cfg(test)]
mod tests {
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::{Plugin, ProcessOutput};

    use super::*;

    const CHART: &str = "apiVersion: v2\nname: web\nversion: 1.0.0\n";

    fn doc(text: &str) -> Document {
        Document { path: "/p/web/Chart.yaml".into(), language: "yaml".into(), version: 1, text: text.into() }
    }

    fn apply(text: &str, action: &CodeAction) -> String {
        let edit = &action.edits[0].1[0];
        let start = openidea_plugin::text::offset(text, edit.range.start).unwrap();
        let end = openidea_plugin::text::offset(text, edit.range.end).unwrap();
        format!("{}{}{}", &text[..start], edit.text, &text[end..])
    }

    fn at(line: u32) -> Range {
        Range::new(Position::new(line, 0), Position::new(line, 0))
    }

    fn host_with_repos() -> MockHost {
        let host = MockHost::install().with_capabilities(&["process:helm", "storage"]);
        host.on_process(HELM, |_, _| {
            Ok(ProcessOutput {
                status: Some(0),
                stdout: "[{\"name\":\"bitnami\",\"url\":\"https://charts.bitnami.com/bitnami\"},{\"name\":\"ingress\",\"url\":\"https://kubernetes.github.io/ingress-nginx\"}]".into(),
                stderr: String::new(),
            })
        });
        host
    }

    #[test]
    fn a_chart_without_dependencies_gets_the_key_and_a_stanza() {
        let _host = host_with_repos();
        let actions = code_actions(&doc(CHART), at(0));
        assert_eq!(actions[0].title, ADD_DEPENDENCY);
        assert_eq!(
            apply(CHART, &actions[0]),
            format!("{CHART}dependencies:\n  - name: NAME\n    version: \"VERSION\"\n    repository: \"REPOSITORY\"\n")
        );
    }

    #[test]
    fn a_stanza_joins_an_existing_block_with_its_indent() {
        let _host = host_with_repos();
        let text = "apiVersion: v2\ndependencies:\n    - name: redis\n      version: 1\n\nicon: x\n";
        let actions = code_actions(&doc(text), at(0));
        assert_eq!(
            apply(text, &actions[0]),
            "apiVersion: v2\ndependencies:\n    - name: redis\n      version: 1\n    - name: NAME\n      version: \"VERSION\"\n      repository: \"REPOSITORY\"\n\nicon: x\n"
        );
    }

    #[test]
    fn a_file_without_a_final_newline_and_crlf_files_are_kept_consistent() {
        let _host = host_with_repos();
        let no_newline = "apiVersion: v2\nname: web";
        assert!(apply(no_newline, &code_actions(&doc(no_newline), at(0))[0]).starts_with("apiVersion: v2\nname: web\ndependencies:\n"));
        let crlf = "apiVersion: v2\r\nname: web\r\n";
        assert!(apply(crlf, &code_actions(&doc(crlf), at(0))[0]).contains("dependencies:\r\n  - name: NAME\r\n"));
    }

    #[test]
    fn nothing_is_offered_for_other_files_v1_charts_or_inline_dependencies() {
        let _host = host_with_repos();
        let mut other = doc(CHART);
        other.path = "/p/web/values.yaml".into();
        assert!(code_actions(&other, at(0)).is_empty());
        assert!(code_actions(&doc("apiVersion: v1\nname: web\n"), at(0)).is_empty());
        assert!(code_actions(&doc("apiVersion: v2\ndependencies: []\n"), at(0)).is_empty());
    }

    #[test]
    fn a_cached_url_becomes_an_alias_and_back() {
        let host = host_with_repos();
        KubeExplorer::default().run_command("helm-repo-cache", &host.invocation(), None).unwrap();
        let url = "apiVersion: v2\ndependencies:\n  - name: pg\n    repository: \"https://charts.bitnami.com/bitnami/\"  # db\n";
        let alias = code_actions(&doc(url), at(3));
        assert_eq!(alias[1].title, "Use repository alias @bitnami");
        assert_eq!(apply(url, &alias[1]), url.replace("\"https://charts.bitnami.com/bitnami/\"", "\"@bitnami\""));

        let aliased = "apiVersion: v2\ndependencies:\n  - name: pg\n    repository: '@bitnami'\n";
        let back = code_actions(&doc(aliased), at(3));
        assert_eq!(back[1].title, "Use repository URL https://charts.bitnami.com/bitnami");
        assert!(apply(aliased, &back[1]).contains("repository: \"https://charts.bitnami.com/bitnami\""));
    }

    #[test]
    fn unknown_repositories_and_other_lines_offer_no_conversion() {
        let host = host_with_repos();
        KubeExplorer::default().run_command("helm-repo-cache", &host.invocation(), None).unwrap();
        let text = "apiVersion: v2\ndependencies:\n  - name: pg\n    repository: https://elsewhere.example\n  - name: x\n    repository: \"@gone\"\n";
        assert_eq!(code_actions(&doc(text), at(3)).len(), 1);
        assert_eq!(code_actions(&doc(text), at(5)).len(), 1);
        assert_eq!(code_actions(&doc(text), at(2)).len(), 1);
    }

    #[test]
    fn without_a_cache_there_is_no_conversion_and_the_command_handles_denial_and_failure() {
        let host = MockHost::install().with_capabilities(&["storage"]);
        let text = "apiVersion: v2\ndependencies:\n  - name: pg\n    repository: https://charts.bitnami.com/bitnami\n";
        assert_eq!(code_actions(&doc(text), at(3)).len(), 1);

        KubeExplorer::default().run_command("helm-repo-cache", &host.invocation(), None).unwrap();
        assert_eq!(host.notifications().len(), 1);

        let host = MockHost::install().with_capabilities(&["process:helm", "storage"]);
        host.on_process(HELM, |_, _| Ok(ProcessOutput { status: Some(1), stdout: String::new(), stderr: "Error: no repositories to show".into() }));
        KubeExplorer::default().run_command("helm-repo-cache", &host.invocation(), None).unwrap();
        assert_eq!(code_actions(&doc(text), at(3)).len(), 1);
    }
}
