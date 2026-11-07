//! Completion in YAML: `k*` abbreviations that expand to a manifest in an
//! empty document, and inside a Helm `{{ }}` action the chart's `.Values`
//! keys, its `define` names after `include`, and `incl`.

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document};
use openidea_plugin::text::offset;
use openidea_plugin::{Position, Range};

use crate::helm::{self, Node};

const ABBREVIATIONS: &[(&str, &str, &str)] = &[
    ("kpod", "Pod", include_str!("../templates/pod.yaml")),
    ("kdeploy", "Deployment", include_str!("../templates/deployment.yaml")),
    ("ksts", "StatefulSet", include_str!("../templates/statefulset.yaml")),
    ("kds", "DaemonSet", include_str!("../templates/daemonset.yaml")),
    ("kjob", "Job", include_str!("../templates/job.yaml")),
    ("kcronjob", "CronJob", include_str!("../templates/cronjob.yaml")),
    ("ksvc", "Service", include_str!("../templates/service.yaml")),
    ("king", "Ingress", include_str!("../templates/ingress.yaml")),
    ("kcm", "ConfigMap", include_str!("../templates/configmap.yaml")),
    ("ksecret", "Secret", include_str!("../templates/secret.yaml")),
    ("kns", "Namespace", include_str!("../templates/namespace.yaml")),
    ("krole", "Role", include_str!("../templates/role.yaml")),
    ("krb", "RoleBinding", include_str!("../templates/rolebinding.yaml")),
    ("kcr", "ClusterRole", include_str!("../templates/clusterrole.yaml")),
    ("kcrb", "ClusterRoleBinding", include_str!("../templates/clusterrolebinding.yaml")),
];

const MAX_SUGGESTIONS: usize = 200;

fn snippet(template: &str) -> String {
    let body = template.replacen("$NAME$", "${1:name}", 1).replace("$NAME$", "$1");
    body.replace("$END$", "$0")
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.' || c == '$'
}

fn inside_action(before: &str) -> Option<usize> {
    let open = before.rfind("{{")?;
    before[open..].find("}}").is_none().then_some(open)
}

fn is_empty_document(text: &str, line: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    let start = (0..line).rev().find(|i| lines[*i].starts_with("---")).map_or(0, |i| i + 1);
    let end = (line + 1..lines.len()).find(|i| lines[*i].starts_with("---")).unwrap_or(lines.len());
    (start..end).filter(|i| *i != line).all(|i| {
        let content = lines[i].trim();
        content.is_empty() || content.starts_with('#')
    })
}

fn manifest_items(doc: &Document, at: Position) -> Vec<CompletionItem> {
    let Some(line) = doc.text.lines().nth(at.line as usize) else { return Vec::new() };
    let typed: String = line.chars().take(at.character as usize).collect();
    let rest: String = line.chars().skip(at.character as usize).collect();
    if typed.is_empty() || !typed.starts_with('k') || !rest.trim().is_empty() || !is_empty_document(&doc.text, at.line as usize) {
        return Vec::new();
    }
    let replace = Range::new(Position::new(at.line, 0), at);
    ABBREVIATIONS
        .iter()
        .filter(|(abbreviation, _, _)| abbreviation.starts_with(&typed))
        .map(|(abbreviation, kind, template)| {
            let mut item = CompletionItem::new(abbreviation, CompletionKind::Snippet);
            item.detail = Some(format!("Kubernetes {kind}"));
            item.insert_text = snippet(template);
            item.snippet = true;
            item.replace = Some(replace);
            item
        })
        .collect()
}

fn chart_dir(path: &str) -> Option<String> {
    let (_, project) = crate::project_of(path)?;
    let chart = crate::chart_file(path)?;
    project.read_file(&chart).ok()?;
    chart.strip_suffix("/Chart.yaml").map(str::to_owned)
}

fn read_in_chart(path: &str, relative: &str) -> Option<String> {
    let (_, project) = crate::project_of(path)?;
    project.read_file(&format!("{}/{relative}", chart_dir(path)?)).ok()
}

fn value_items(path: &str, token: &str, replace: Range) -> Vec<CompletionItem> {
    let Some((_, typed)) = token.split_once(".Values.") else { return Vec::new() };
    let Some(Node::Map(root)) = read_in_chart(path, "values.yaml").and_then(|text| helm::parse_values(&text)) else {
        return Vec::new();
    };
    let mut parts: Vec<&str> = typed.split('.').collect();
    let partial = parts.pop().unwrap_or("");
    let mut children = &root;
    for part in parts {
        match children.get(part) {
            Some(Node::Map(next)) => children = next,
            _ => return Vec::new(),
        }
    }
    children
        .iter()
        .filter(|(key, _)| key.starts_with(partial))
        .take(MAX_SUGGESTIONS)
        .map(|(key, node)| {
            let mut item = CompletionItem::new(key, CompletionKind::Property);
            item.detail = Some(match node {
                Node::Map(_) => "values.yaml section".to_owned(),
                Node::Open => "values.yaml".to_owned(),
            });
            item.replace = Some(replace);
            item
        })
        .collect()
}

fn include_items(doc: &Document, partial: &str, replace: Range) -> Vec<CompletionItem> {
    let mut names: std::collections::BTreeSet<String> = helm::defined_names(&doc.text).collect();
    if let Some(helpers) = read_in_chart(&doc.path, "templates/_helpers.tpl") {
        names.extend(helm::defined_names(&helpers));
    }
    names
        .into_iter()
        .filter(|name| name.starts_with(partial))
        .take(MAX_SUGGESTIONS)
        .map(|name| {
            let mut item = CompletionItem::new(&name, CompletionKind::Function);
            item.detail = Some("define".to_owned());
            item.replace = Some(replace);
            item
        })
        .collect()
}

fn action_items(doc: &Document, at: Position, action: &str) -> Vec<CompletionItem> {
    if action.matches(['"', '`']).count() % 2 == 1 {
        let quote = action.rfind(['"', '`']).unwrap_or(0);
        let head = action[..quote].trim_end();
        if !(head.ends_with("include") || head.ends_with("template")) {
            return Vec::new();
        }
        let partial = &action[quote + 1..];
        let replace = Range::new(Position::new(at.line, at.character - partial.chars().count() as u32), at);
        return include_items(doc, partial, replace);
    }
    let token: String = action.chars().rev().take_while(|c| is_word(*c)).collect::<Vec<_>>().into_iter().rev().collect();
    if token.contains(".Values.") {
        let partial = token.rsplit('.').next().unwrap_or("");
        let replace = Range::new(Position::new(at.line, at.character - partial.chars().count() as u32), at);
        return value_items(&doc.path, &token, replace);
    }
    if token.is_empty() || !"incl".starts_with(&token) {
        return Vec::new();
    }
    let mut item = CompletionItem::new("incl", CompletionKind::Snippet);
    item.detail = Some("include \"name\" .".to_owned());
    item.insert_text = "include \"${1:name}\" ${2:.}$0".to_owned();
    item.snippet = true;
    item.replace = Some(Range::new(Position::new(at.line, at.character - token.chars().count() as u32), at));
    vec![item]
}

pub fn items(doc: &Document, at: Position) -> Vec<CompletionItem> {
    let Some(caret) = offset(&doc.text, at) else { return Vec::new() };
    match inside_action(&doc.text[..caret]) {
        Some(open) => action_items(doc, at, &doc.text[open + 2..caret]),
        None => manifest_items(doc, at),
    }
}

#[cfg(test)]
mod tests {
    use openidea_plugin::mock::MockHost;

    use super::*;

    fn doc(text: &str) -> Document {
        Document { path: "/p/chart/templates/a.yaml".into(), language: "yaml".into(), version: 1, text: text.into() }
    }

    fn at_end(text: &str) -> Position {
        let line = text.lines().count() as u32 - 1;
        Position::new(line, text.lines().last().map_or(0, |l| l.chars().count()) as u32)
    }

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    fn project() -> MockHost {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        host.set_project_root(Some("/p"));
        host.add_file("chart/Chart.yaml", "apiVersion: v2\nname: c\nversion: 1\n");
        host.add_file("chart/values.yaml", "image:\n  repository: nginx\n  tag: 1\nreplicas: 2\n");
        host.add_file("chart/templates/_helpers.tpl", "{{- define \"c.name\" -}}c{{- end -}}\n{{- define \"c.labels\" -}}l{{- end -}}\n");
        host
    }

    #[test]
    fn an_abbreviation_in_an_empty_file_offers_a_manifest() {
        let _host = project();
        let text = "kdep";
        let found = items(&doc(text), at_end(text));
        assert_eq!(labels(&found), ["kdeploy"]);
        assert!(found[0].snippet);
        assert!(found[0].insert_text.contains("name: ${1:name}") && found[0].insert_text.contains("app: $1") && found[0].insert_text.contains("$0"));
        assert!(!found[0].insert_text.contains("$NAME$"));
        assert_eq!(found[0].replace.unwrap().start, Position::new(0, 0));
    }

    #[test]
    fn k_alone_offers_all_fifteen() {
        let _host = project();
        assert_eq!(items(&doc("k"), Position::new(0, 1)).len(), 15);
    }

    #[test]
    fn a_comment_or_a_new_document_still_counts_as_empty() {
        let _host = project();
        let text = "# notes\n---\nkpod";
        assert_eq!(labels(&items(&doc(text), at_end(text))), ["kpod"]);
    }

    #[test]
    fn nothing_is_offered_inside_a_manifest_or_other_yaml() {
        let _host = project();
        for text in ["apiVersion: v1\nkind: Pod\nk", "services:\n  web:\n    image: x\nk", "name: x\n  kpod"] {
            assert!(items(&doc(text), at_end(text)).is_empty(), "{text}");
        }
    }

    #[test]
    fn a_second_document_is_judged_on_its_own() {
        let _host = project();
        let text = "apiVersion: v1\nkind: Pod\n---\nksvc";
        assert_eq!(labels(&items(&doc(text), at_end(text))), ["ksvc"]);
    }

    #[test]
    fn values_keys_complete_after_dot_values() {
        let _host = project();
        let text = "a: {{ .Values.";
        assert_eq!(labels(&items(&doc(text), at_end(text))), ["image", "replicas"]);
        let text = "a: {{ .Values.image.re";
        let found = items(&doc(text), at_end(text));
        assert_eq!(labels(&found), ["repository"]);
        assert_eq!(found[0].replace.unwrap().start.character, 20);
    }

    #[test]
    fn unknown_value_paths_and_non_chart_files_offer_nothing() {
        let host = project();
        let text = "a: {{ .Values.nope.";
        assert!(items(&doc(text), at_end(text)).is_empty());
        host.set_project_root(Some("/other"));
        let text = "a: {{ .Values.";
        assert!(items(&doc(text), at_end(text)).is_empty());
    }

    #[test]
    fn include_offers_the_defined_names() {
        let _host = project();
        let text = "{{- define \"c.local\" -}}x{{- end }}\na: {{ include \"c.";
        let found = items(&doc(text), at_end(text));
        assert_eq!(labels(&found), ["c.labels", "c.local", "c.name"]);
        let text = "a: {{ print \"c.";
        assert!(items(&doc(text), at_end(text)).is_empty());
    }

    #[test]
    fn incl_expands_to_an_include_inside_an_action_only() {
        let _host = project();
        let text = "a: {{ inc";
        let found = items(&doc(text), at_end(text));
        assert_eq!(labels(&found), ["incl"]);
        assert!(found[0].insert_text.starts_with("include \"${1:name}\""));
        let text = "a: {{ inc }} k";
        assert!(items(&doc(text), at_end(text)).is_empty());
    }
}
