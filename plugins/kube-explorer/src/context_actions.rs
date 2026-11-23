//! Context menus use the clicked row's scoped identity, never a previous selection.
use super::*;
use serde_json::{Value, json};

type Action = (&'static str, &'static str, &'static str);

impl KubeExplorer {
    fn row_actions(&self, item: &TreeItem) -> Result<Vec<Action>, Error> {
        let mut actions = Vec::new();
        if self.checked_tree_key(&item.key)? == "kube:status" {
            return Ok(vec![("tree-import-config","Import Kubeconfig Privately…","paste"),("tree-review-config","Review Imported Kubeconfigs…","list")]);
        }
        if let Some((parent, name)) = self.checked_choice(&item.key)? {
            let active = if parent == "ctx" { self.context.as_ref().or(self.current_context.as_ref()) } else { self.namespace.as_ref() };
            if active.map(String::as_str) != Some(name.as_str()) {
                actions.push(("tree-switch", if parent == "ctx" {"Switch Context"} else {"Switch Namespace"}, "check"));
            }
        } else if let Some((kind, _)) = self.checked_resource(&item.key)? {
            if self.rich_owner.is_some() {
                actions.extend([("resource-describe", "Describe", "info"), ("resource-yaml", "Edit YAML…", "file-yaml")]);
            }
            if kind == "pods" || WORKLOADS.contains(&kind.as_str()) {
                actions.extend([("resource-logs", "Follow Logs (Terminal)", "file-log"), ("resource-log-view", "Log Snapshot…", "filter"), ("resource-log-sources", "Follow Logs from Selected Sources…", "settings")]);
            }
            if kind == "pods" {
                actions.extend([("resource-shell", "Open Shell", "terminal"), ("resource-attach", "Attach", "attach-to-process"), ("resource-debug", "Debug…", "debug")]);
            }
            if FORWARDABLE.iter().any(|(k, _)| *k == kind) { actions.push(("resource-port-forward", "Port Forward…", "external-link")); }
            if kind == "secrets" && self.rich_owner.is_some() {
                actions.extend([("resource-show-secret", "Show Secret…", "eye"), ("resource-edit-secret", "Edit Secret…", "edit")]);
            }
            actions.push(("resource-delete", "Delete…", "trash"));
        } else if item.expandable {
            actions.push(if item.expanded {("tree-collapse", "Collapse", "chevron-up")} else {("tree-expand", "Expand", "chevron-down")});
            actions.push(("tree-refresh", "Refresh", "refresh"));
            if self.checked_tree_key(&item.key)? == "kube:cluster" {
                if self.process_scope.root().is_some() && !self.all_namespaces() {actions.push(("tree-watch", "Watch Resources…", "eye"));}
                actions.extend([("tree-open-watch", "Open Resource Watch", "list"),("tree-kubeconfig", "Choose Kubeconfig…", "folder-open"),
                    ("tree-import-config", "Import Kubeconfig Privately…", "paste"),("tree-review-config", "Review Imported Kubeconfigs…", "list")]);
            }
            if self.checked_tree_key(&item.key)? == "kind:secrets" && !self.all_namespaces() && self.rich_owner.is_some() {
                actions.push(("tree-create-secret", "Create Secret…", "plus"));
            }
        }
        Ok(actions)
    }

    pub(super) fn context_menu_config(&mut self, view: &str, ctx: &Invocation) -> Result<String, Error> {
        let mut config: Value = serde_json::from_str(include_str!("ui_config.json")).map_err(|_|Error::Invalid("Invalid UI configuration".into()))?;
        if view != PANEL { return Ok(config.to_string()); }
        let current = self.render(view, None, ctx)?;
        config["actions"] = json!(self.menu_actions(view, &current)?.unwrap_or_default());
        Ok(config.to_string())
    }

    pub(super) fn menu_actions(&self, view: &str, current: &View) -> Result<Option<Vec<Value>>, Error> {
        if view != PANEL { return Ok(None); }
        let Body::Tree(items) = &current.body else { return Ok(Some(Vec::new())); };
        let mut actions: Vec<Value> = Vec::new();
        for item in items {
            for (id, label, icon) in self.row_actions(item)? {
                if let Some(action) = actions.iter_mut().find(|action|action["id"] == id) {
                    action["keys"].as_array_mut().unwrap().push(json!(item.key));
                } else {
                    actions.push(json!({"view":view,"id":id,"label":label,"icon":icon,"kind":"","keys":[item.key],"replacesOpen":matches!(id,"resource-describe"|"tree-switch"|"tree-expand"|"tree-collapse")}));
                }
            }
        }
        // Match native menu order regardless of which tree kind was visited first.
        actions.sort_by_key(|action| {
            ["tree-switch", "tree-expand", "tree-collapse", "tree-refresh",
             "tree-create-secret", "tree-watch", "tree-open-watch", "tree-kubeconfig",
             "tree-import-config", "tree-review-config", "resource-describe",
             "resource-yaml", "resource-logs", "resource-log-view", "resource-log-sources",
             "resource-shell", "resource-attach", "resource-debug", "resource-port-forward",
             "resource-show-secret", "resource-edit-secret", "resource-delete"]
                .iter().position(|id| Some(*id) == action["id"].as_str()).unwrap_or(usize::MAX)
        });
        Ok(Some(actions))
    }

    pub(super) fn tree_context_action(&mut self, id: &str, key: Option<&str>, ctx: &Invocation) -> Result<(), Error> {
        let key = key.ok_or_else(|| Error::Invalid("Select a resource in the Kube Explorer window first.".into()))?;
        let current = self.render(TREE, None, ctx)?;
        let Body::Tree(items) = current.body else { return Err(Error::Stale); };
        let payload=self.service_payload(key)?;
        let item = items.iter().find(|item|self.service_payload(&item.key).is_ok_and(|current|current == payload)).ok_or_else(|| Error::Invalid("This resource is no longer listed. Refresh the group.".into()))?;
        if !self.row_actions(item)?.iter().any(|action|action.0 == id) { return Err(Error::Stale); }
        let bare = self.checked_tree_key(key)?;
        match id {
            "tree-switch" => self.handle_tree(ViewEvent::Activate(bare), ctx)?,
            "tree-expand" => self.handle_tree(ViewEvent::Expand(bare), ctx)?,
            "tree-collapse" => self.handle_tree(ViewEvent::Collapse(bare), ctx)?,
            "tree-refresh" => {
                if bare == "kube:cluster" { self.reload(); }
                else { let loaded = self.load(&bare); self.children.insert(bare, loaded); crate::refresh_views(); }
            }
            "tree-watch" | "tree-open-watch" | "tree-kubeconfig" | "tree-import-config" | "tree-review-config" => {
                let command=match id {"tree-watch"=>"watch-resources","tree-open-watch"=>"open-resource-watch","tree-kubeconfig"=>"choose-kubeconfig","tree-import-config"=>"import-pasted-kubeconfig",_=>"review-imported-kubeconfigs"};
                if let Some(message)=self.run_command(command,ctx,None)? {host::notify(NotificationLevel::Info,"Kube Explorer",&message);}
            }
            "tree-create-secret" => { self.button_in("create-secret", Some(ctx))?; crate::ui_group::reveal_view(PANEL)?; }
            _ => return Err(Error::Stale),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    fn fixture() -> (MockHost, KubeExplorer) {
        let host = MockHost::install().with_capabilities(&["project.read", "process:kubectl", "storage"]);
        host.on_process(KUBECTL, |_, _| Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: "local\n".into(), stderr: String::new() }));
        let mut plugin = KubeExplorer::default();
        plugin.render(TREE, None, &host.invocation()).unwrap();
        plugin.current_context = Some("local".into());
        for (group, names) in [("ctx",vec!["local","remote"]),("ns",vec!["team"]),("kind:pods",vec!["web"]),("kind:deployments",vec!["api"]),("kind:services",vec!["api"]),("kind:secrets",vec!["token"]),("kind:configmaps",vec!["config"])] {
            plugin.expanded.insert(group.into());plugin.children.insert(group.into(),Ok(names.into_iter().map(str::to_owned).collect()));
        }
        (host, plugin)
    }
    #[test]
    fn resource_describe_without_selection_explains() {
        let (host, mut plugin) = fixture();
        for id in ["resource-describe", "tree-refresh"] {
            let error = plugin.run_command(id, &host.invocation(), None).unwrap_err();
            assert!(matches!(error, Error::Invalid(ref message) if message == "Select a resource in the Kube Explorer window first."), "{id}: {error:?}");
        }
    }
    fn request(host: &MockHost, plugin: &mut KubeExplorer, operation: Value) -> Value {
        let mut request = operation;
        if request.get("targetView").is_none(){request["targetView"]=json!(PANEL);}request["channel"]=json!("ui-kit");request["sequence"]=json!(1);
        request["project"]=json!(host.invocation().project().unwrap().root().unwrap());
        plugin.handle(PANEL, ViewEvent::Message(request.to_string()), &host.invocation()).unwrap();
        serde_json::from_str(&host.view_messages().last().unwrap().1).unwrap()
    }
    fn row(model: &Value, label: &str) -> String {
        model["body"]["items"].as_array().unwrap().iter().find(|item| item["label"]==label).unwrap()["key"].as_str().unwrap().into()
    }
    fn actions(model: &Value, key: &str) -> Vec<String> {
        model["actions"].as_array().unwrap().iter().filter(|action|action["keys"].as_array().unwrap().iter().any(|value|value==key)).map(|action|action["id"].as_str().unwrap().to_owned()).collect()
    }
    #[test]
    fn menus_follow_each_clicked_kind_without_selecting_or_querying() {
        let (host, mut plugin)=fixture();let before=host.process_runs().len();
        let answer=request(&host,&mut plugin,json!({"op":"render"}));let model=&answer["model"];
        let pod=actions(model,&row(model,"web"));assert!(pod.contains(&"resource-shell".into())&&pod.contains(&"resource-debug".into())&&pod.contains(&"resource-logs".into()));
        let service_key=model["body"]["items"].as_array().unwrap().iter().find(|item|item["key"].as_str().unwrap().starts_with("res:services/")).unwrap()["key"].as_str().unwrap();
        let service=actions(model,service_key);assert!(service.contains(&"resource-port-forward".into()));assert!(!service.contains(&"resource-shell".into())&&!service.contains(&"resource-logs".into()));
        let secret=actions(model,&row(model,"token"));assert!(secret.contains(&"resource-show-secret".into())&&secret.contains(&"resource-edit-secret".into()));assert!(!secret.contains(&"resource-shell".into()));
        let config=actions(model,&row(model,"config"));assert_eq!(config,["resource-describe","resource-yaml","resource-delete"]);
        assert!(actions(model,&row(model,"local")).is_empty());assert_eq!(actions(model,&row(model,"remote")),["tree-switch"]);
        assert_eq!(actions(model,&row(model,"team")),["tree-switch"]);
        assert!(actions(model,&row(model,"Secrets")).contains(&"tree-create-secret".into()));
        assert!(plugin.selected.is_none());assert_eq!(host.process_runs().len(),before);
    }
    #[test]
    fn group_actions_refresh_the_returned_menu_and_wrong_kind_commands_are_denied() {
        let (host, mut plugin)=fixture();let answer=request(&host,&mut plugin,json!({"op":"render"}));let model=&answer["model"];
        let group=row(model,"Pods");
        let answer=request(&host,&mut plugin,json!({"op":"command","id":"tree-collapse","key":group}));assert!(answer.get("error").is_none());
        let group=row(&answer["model"],"Pods");assert_eq!(actions(&answer["model"],&group),["tree-expand","tree-refresh"]);
        let secret=row(&answer["model"],"token");let before=host.process_runs().len();
        let denied=request(&host,&mut plugin,json!({"op":"command","id":"resource-shell","key":secret}));assert!(denied.get("error").is_some());assert_eq!(host.process_runs().len(),before);assert!(host.terminals().is_empty());
    }
    #[test]
    fn custom_group_watch_navigation_preserves_services_and_rejects_hidden_panel_callbacks() {
        let (host,mut plugin)=fixture();
        let answer=request(&host,&mut plugin,json!({"op":"render"}));
        let key=row(&answer["model"],"web");let before=host.process_runs().len();
        let watch=request(&host,&mut plugin,json!({"op":"select-view","target":watch::VIEW}));
        assert_eq!(watch["model"]["activeView"],watch::VIEW);
        assert_eq!(watch["model"]["viewTabs"].as_array().unwrap().len(),2);
        let stale=request(&host,&mut plugin,json!({"op":"command","id":"resource-shell","key":key}));
        assert!(stale.get("error").is_some());assert!(host.terminals().is_empty());
        let back=request(&host,&mut plugin,json!({"op":"select-view","target":PANEL,"targetView":watch::VIEW}));
        assert_eq!(back["model"]["activeView"],PANEL);
        assert_eq!(host.process_runs().len(),before);
        let native=plugin.render(TREE,None,&host.invocation()).unwrap();
        assert!(matches!(native.body,Body::Tree(_)));
    }

    #[test]
    fn a_vanished_row_asks_to_refresh_the_group() {
        let (host,mut plugin)=fixture();
        let answer=request(&host,&mut plugin,json!({"op":"render"}));let target=row(&answer["model"],"remote");
        plugin.children.insert("ctx".into(),Ok(vec!["local".into()]));
        let error=plugin.run_command("tree-switch",&host.invocation(),Some(&target)).unwrap_err();
        assert_eq!(error,Error::Invalid("This resource is no longer listed. Refresh the group.".into()));
    }

    #[test]
    fn context_switch_invalidates_old_keys_and_uses_the_clicked_context() {
        let (host,mut plugin)=fixture();let answer=request(&host,&mut plugin,json!({"op":"render"}));let target=row(&answer["model"],"remote");
        let answer=request(&host,&mut plugin,json!({"op":"command","id":"tree-switch","key":target}));assert!(answer.get("error").is_none());assert_eq!(plugin.context.as_deref(),Some("remote"));
        let before=host.process_runs().len();let stale=request(&host,&mut plugin,json!({"op":"command","id":"tree-switch","key":target}));assert!(stale.get("error").is_some());assert_eq!(host.process_runs().len(),before);
    }
}
