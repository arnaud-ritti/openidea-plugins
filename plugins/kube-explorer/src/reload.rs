//! Reload from Cluster: replaces each Kubernetes document of the open file with the live object.
//! A command, so it runs outside the 50 ms provider budget, and edits through `editor.write`.

use openidea_plugin::{Error, Invocation, NotificationLevel, Position, Range, TextEdit, host};

use crate::manifest::{self, Doc};
use crate::{KubeExplorer, REQUEST_TIMEOUT, is_dns_subdomain, without_resource_version};

fn position(text: &str, offset: usize) -> Position {
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let column = before.rsplit('\n').next().map_or(0, |last| last.chars().count());
    Position::new(line as u32, column as u32)
}

fn resource_type(doc: &Doc) -> Option<String> {
    let kind = doc.kind.as_ref()?.text.to_lowercase();
    if kind.is_empty() || !kind.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    let group = doc.api_version.as_ref()?.text.split_once('/').map(|(group, _)| group);
    Some(match group {
        Some(group) if is_dns_subdomain(group) => format!("{kind}.{group}"),
        _ => kind,
    })
}

fn leading_comments(text: &str) -> &str {
    let mut end = 0;
    for line in text.split_inclusive('\n') {
        if !line.trim().is_empty() && !line.trim_start().starts_with('#') {
            break;
        }
        end += line.len();
    }
    &text[..end]
}

impl KubeExplorer {
    pub fn reload_from_cluster(&self, ctx: &Invocation) -> Result<(), Error> {
        let editor = ctx.editor().ok_or("No manifest is open.")?;
        let document = editor.document()?;
        let mut edits = Vec::new();
        let mut skipped = Vec::new();
        let mut manifests = 0;
        for doc in manifest::documents(&document.text).iter().filter(|doc| doc.is_manifest()) {
            manifests += 1;
            let label = doc.name.as_ref().map_or("a document without a name".to_owned(), |name| name.text.clone());
            match self.live_yaml(doc) {
                Ok(yaml) => {
                    let original = &document.text[doc.range.clone()];
                    let start = doc.range.start + leading_comments(original).len();
                    let range = Range::new(position(&document.text, start), position(&document.text, doc.range.end));
                    edits.push(TextEdit { range, text: yaml });
                }
                Err(error) => skipped.push(format!("{label}: {error}")),
            }
        }
        if manifests == 0 {
            return Err("The open file has no Kubernetes manifest.".into());
        }
        if edits.is_empty() {
            return Err(skipped.join("\n").into());
        }
        let reloaded = edits.len();
        match editor.apply_edits(document.version, &edits) {
            Ok(_) => {}
            Err(Error::Denied(_)) => {
                host::notify(NotificationLevel::Warning, "Reload from cluster", "Allow editor changes for Kube Explorer in Settings › Plugins to reload a manifest.");
                return Ok(());
            }
            Err(error) => return Err(error),
        }
        let detail = if skipped.is_empty() { format!("{reloaded} of {manifests} documents") } else { format!("{reloaded} of {manifests} documents; not reloaded: {}", skipped.join("; ")) };
        crate::config::notify_success("Reloaded from the cluster", &detail);
        Ok(())
    }

    fn live_yaml(&self, doc: &Doc) -> Result<String, Error> {
        let kind = resource_type(doc).ok_or("unreadable kind")?;
        let name = doc.name.as_ref().map(|name| name.text.as_str()).ok_or("no metadata.name")?;
        if !is_dns_subdomain(name) {
            return Err(format!("\"{name}\" is not a resource name").into());
        }
        let in_file = doc.metadata_keys.iter().find(|(key, ..)| key == "namespace").and_then(|(.., value)| value.as_ref());
        let namespace = in_file.map(|v| v.text.clone()).or_else(|| self.namespace.clone().filter(|ns| ns != crate::ALL_NAMESPACES));
        let _task = host::begin_task(&format!("Reading {kind}/{name}"), false);
        let text = self.kubectl_in(namespace.as_deref(), &["get", &kind, name, "-o", "yaml", REQUEST_TIMEOUT], None)?;
        Ok(without_resource_version(&text))
    }
}

#[cfg(test)]
mod tests {
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::ProcessOutput;

    use crate::KUBECTL;

    use super::*;

    const FILE: &str = "# keep me\napiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\n  namespace: shop\nspec:\n  replicas: 1\n---\napiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: gone\n";

    fn fixture() -> (MockHost, std::rc::Rc<std::cell::RefCell<Vec<Vec<String>>>>) {
        let host = MockHost::install().with_capabilities(&["process:kubectl", "editor.read", "editor.write", "storage"]);
        let runs: std::rc::Rc<std::cell::RefCell<Vec<Vec<String>>>> = Default::default();
        let seen = runs.clone();
        host.on_process(KUBECTL, move |args, _| {
            seen.borrow_mut().push(args.to_vec());
            if args.contains(&"configmap".to_owned()) {
                return Ok(ProcessOutput { status: Some(1), stdout: String::new(), stderr: "NotFound\n".into() });
            }
            Ok(ProcessOutput { status: Some(0), stdout: "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\n  resourceVersion: \"9\"\nspec:\n  replicas: 3\n".into(), stderr: String::new() })
        });
        (host, runs)
    }

    #[test]
    fn a_found_document_is_replaced_keeping_its_comments_and_a_missing_one_is_left() {
        let (host, runs) = fixture();
        host.open_editor("/p/web.yaml", "yaml", FILE, &[(0, 0, 0, 0)]);
        KubeExplorer::default().reload_from_cluster(&host.invocation()).unwrap();

        let text = host.editor_text();
        assert!(text.starts_with("# keep me\n") && text.contains("replicas: 3") && !text.contains("resourceVersion"));
        assert!(text.contains("name: gone"));
        assert_eq!(runs.borrow()[0][..3], ["--namespace=shop", "get", "deployment.apps"]);
        assert!(host.notifications()[0].body.contains("1 of 2 documents; not reloaded: gone"));
    }

    #[test]
    fn nothing_found_is_an_error_and_without_editor_write_only_notifies() {
        let (host, _) = fixture();
        host.open_editor("/p/c.yaml", "yaml", "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: gone\n", &[(0, 0, 0, 0)]);
        assert!(KubeExplorer::default().reload_from_cluster(&host.invocation()).is_err());

        let (host, _) = fixture();
        let host = host.with_capabilities(&["process:kubectl", "editor.read", "storage"]);
        host.open_editor("/p/web.yaml", "yaml", FILE, &[(0, 0, 0, 0)]);
        KubeExplorer::default().reload_from_cluster(&host.invocation()).unwrap();
        assert_eq!(host.editor_text(), FILE);
        assert!(host.notifications()[0].body.contains("Allow editor changes"));
    }
}
