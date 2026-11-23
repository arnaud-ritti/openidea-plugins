//! Cooperative indexing: one bounded builder step per host invocation.
use crate::{Main, Index, index, refresh_laravel};
use openidea_plugin::{Error, Invocation, Project, host, LogLevel};
use openidea_plugin::views::{Body, View};

impl Main {
    pub(crate) fn completed_index_for(&self, root: &str) -> Option<&Index> {
        if self.pending_indexes.contains(root) { None } else { self.indexes.get(root) }
    }

    pub(crate) fn index_work_pending(&self, root: &str) -> bool {
        self.pending_indexes.contains(root) || self.index_rechecks.get(root).is_some_and(|queue|!queue.is_empty())
    }

    pub(crate) fn index_pending(&self, root: &str) -> bool {
        self.pending_indexes.contains(root)
    }

    pub(crate) fn queue_index(&mut self, project: &Project) {
        let Ok(root) = project.root() else { return };
        self.runtime_inspection.invalidate_source_views(&root);
        self.model_rows.borrow_mut().remove(&root);
        let had_index=self.indexes.remove(&root).is_some();
        self.index_builders.remove(&root);
        self.index_progress.remove(&root);
        self.index_rechecks.remove(&root);
        self.pending_indexes.insert(root);
        if had_index { refresh_laravel(); }
    }

    /// Queue all scopes without indexing any project during activation.
    pub(crate) fn queue_indexes(&mut self) {
        let projects = host::projects().unwrap_or_default();
        let roots = projects.iter().filter_map(|project| project.root().ok()).collect::<std::collections::BTreeSet<_>>();
        self.model_rows.borrow_mut().clear();
        self.indexes.retain(|root, _| roots.contains(root));
        self.index_builders.retain(|root, _| roots.contains(root));
        self.pending_indexes.retain(|root| roots.contains(root));
        self.index_progress.retain(|root, _| roots.contains(root));
        self.index_rechecks.retain(|root, _| roots.contains(root));
        self.show_console_rules(projects.iter().any(index::is_laravel));
        for project in projects { self.queue_index(&project); }
    }

    /// One bounded step for events, schedules, commands and view ticks.
    pub(crate) fn advance_index(&mut self, project: &Project) {
        let deadline = std::time::Instant::now() + index::STEP_BUDGET;
        let Ok(root) = project.root() else { return };
        if self.indexes.get(&root).is_some_and(|index|!index::index_settings_current(index)) { self.queue_index(project); }
        if !self.pending_indexes.contains(&root) {
            let maintenance=self.index_rechecks.get(&root).is_some_and(|queue|!queue.is_empty()) || self.helper_automation.has_deferred(&root);
            if !maintenance { return; }
            let task=host::begin_task("Laravel",true);
            if task.is_cancelled() { return; }
            if let Some(path)=self.index_rechecks.get_mut(&root).and_then(|queue|queue.pop_front()) {
                self.check(project,&path);
                if let Some((completed,_))=self.index_progress.get_mut(&root) { *completed+=1; }
                if self.index_rechecks.get(&root).is_none_or(|queue|queue.is_empty()) {
                    self.index_rechecks.remove(&root);
                    self.index_progress.remove(&root);
                    refresh_laravel();
                }
                return;
            }
            if self.helper_automation.has_deferred(&root) {
                let result = self.artisan_workspace.runner_for_project(project).and_then(|runner|self.helper_automation.resume_deferred(self,project,runner));
                if let Err(error)=result { host::log(LogLevel::Warn,&format!("Deferred Laravel helper generation for {root}: {error}")); }
            }
            return;
        }
        // A closed or newly foreign scope must never publish an old builder.
        if host::projects().ok().is_none_or(|projects| !projects.iter().any(|project| project.root().ok().as_deref() == Some(root.as_str()))) {
            self.pending_indexes.remove(&root);
            self.index_builders.remove(&root);
            self.index_progress.remove(&root);
            return;
        }
        // SDK indexing tasks use the supported background wall deadline. The
        // per-call fuel limit and bounded atomic builder work stay unchanged.
        let task=host::begin_task(crate::ui_text::text("root.indexing","Indexing Laravel projects"),true);
        let (completed,total)=self.index_progress.get(&root).copied().unwrap_or((0,0));
        task.report(completed.min(u32::MAX as usize) as u32,(total>0).then_some(total.min(u32::MAX as usize) as u32),None);
        if task.is_cancelled() { return; }
        if !self.index_builders.contains_key(&root) {
            if !index::is_laravel(project) {
                self.pending_indexes.remove(&root);
                self.runtime_inspection.invalidate(&root);
                return;
            }
            // Bound retained snapshots without starving a newly selected scope.
            // Evicted scopes stay queued and restart when selected again.
            if self.index_builders.len() >= 8 {
                if let Some(evicted)=self.index_builders.keys().find(|other|*other!=&root).cloned() {
                    self.index_builders.remove(&evicted);
                    self.index_progress.remove(&evicted);
                }
            }
            match index::Builder::new(project) {
                Ok(builder) => { self.index_builders.insert(root.clone(), builder); }
                Err(error) => { self.fail_index(&root, &error); return; }
            }
        }
        let result = self.index_builders.get_mut(&root).expect("queued builder").step(project, deadline);
        match result {
            Ok(index::BuildProgress::Pending { completed, total }) => {
                task.report(completed.min(u32::MAX as usize) as u32,Some(total.min(u32::MAX as usize) as u32),None);
                self.index_progress.insert(root, (completed, total));
            }
            Ok(index::BuildProgress::Ready(index)) => self.publish_index(project, root, index),
            Err(Error::Stale) => self.queue_index(project),
            Err(error) => self.fail_index(&root, &error),
        }
    }

    fn fail_index(&mut self, root: &str, error: &Error) {
        self.index_builders.remove(root);
        self.index_progress.remove(root);
        self.pending_indexes.remove(root);
        host::log(LogLevel::Warn, &format!("Laravel indexing failed for {root}: {error}"));
        refresh_laravel();
    }

    fn publish_index(&mut self, _project: &Project, root: String, index: Index) {
        self.index_builders.remove(&root);
        self.index_progress.remove(&root);
        self.pending_indexes.remove(&root);
        self.model_rows.borrow_mut().remove(&root);
        self.indexes.insert(root.clone(), index);
        self.show_console_rules(true);
        let _ = host::set_context("indexed", true);
        let checked = self.checked.iter().filter(|path| crate::under(path, &root)).cloned().collect::<Vec<_>>();
        if !checked.is_empty() {
            self.index_progress.insert(root.clone(),(0,checked.len()));
            self.index_rechecks.insert(root,checked.into());
        }
        refresh_laravel();
    }

    pub(crate) fn advance_document_index(&mut self, path: &str) {
        if let Some(project) = crate::project_for(path) { self.advance_index_from_provider(&project); }
    }

    /// Editor providers only continue a builder an event already started, through
    /// file reads under `PROVIDER_STEP_BUDGET`: no builder creation, listing,
    /// heavy jobs, capture, rechecks or deferred automation.
    fn advance_index_from_provider(&mut self, project: &Project) {
        let Ok(root) = project.root() else { return };
        if self.indexes.get(&root).is_some_and(|index|!index::index_settings_current(index)) { self.queue_index(project); }
        if !self.pending_indexes.contains(&root) { return; }
        let Some(builder) = self.index_builders.get_mut(&root) else { return };
        let deadline = std::time::Instant::now() + index::PROVIDER_STEP_BUDGET;
        match builder.provider_step(project, deadline) {
            Ok(index::BuildProgress::Pending { completed, total }) => { self.index_progress.insert(root, (completed, total)); }
            Ok(index::BuildProgress::Ready(index)) => self.publish_index(project, root, index),
            Err(Error::Stale) => self.queue_index(project),
            Err(error) => self.fail_index(&root, &error),
        }
    }

    pub(crate) fn advance_context_index(&mut self, ctx: &Invocation) {
        if let Some(project) = ctx.project() { self.advance_index(&project); }
    }

    /// Background work changes must not replace independent forms or processes.
    pub(crate) fn decorate_index_progress(&self, mut view:View, ctx:&Invocation)->View {
        let pending=ctx.project().and_then(|project|project.root().ok()).is_some_and(|root|self.index_work_pending(&root));
        if !pending { return view; }
        let poll=serde_json::json!({"intervalMs":100,"message":{"op":"index-tick"}});
        view.body=match view.body {
            Body::Html(html)=>Body::Other{kind:"html".into(),data:serde_json::json!({"html":html,"backgroundPoll":poll}).to_string()},
            Body::Message(text)=>Body::Other{kind:"message".into(),data:serde_json::json!({"text":text,"backgroundPoll":poll}).to_string()},
            Body::Other{kind,data}=>{
                match serde_json::from_str::<serde_json::Value>(&data) {
                    Ok(mut value) if value.is_object()=>{value["backgroundPoll"]=poll;Body::Other{kind,data:value.to_string()}},
                    _=>Body::Other{kind,data}
                }
            },
            other=>other,
        };
        let progress=crate::ui_text::text("index.building","Indexing Laravel project…");
        view.status=Some(match view.status {Some(status)=>format!("{status} · {progress}"),None=>progress.into()});
        view
    }

    pub(crate) fn pending_index_view(&self, ctx: &Invocation) -> Option<View> {
        let root = ctx.project()?.root().ok()?;
        if !self.index_work_pending(&root) { return None; }
        let (completed, total) = self.index_progress.get(&root).copied().unwrap_or_default();
        let text = crate::ui_text::text("index.building", "Indexing Laravel project…");
        let status = crate::ui_text::format("index.progress", "{completed} of {total} indexing tasks complete", &[("completed", &completed.to_string()), ("total", &total.to_string())]);
        Some(View::new(Body::Other { kind: "message".into(), data: serde_json::json!({
            "text": text,
            "backgroundPoll": {"intervalMs": 100, "message": {"op": "index-tick"}}
        }).to_string() }).status(&status))
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use openidea_plugin::mock::MockHost;

    #[test]
    fn provider_advance_on_a_fresh_project_creates_no_builder() {
        let host=MockHost::install();
        host.add_file("composer.json",r#"{"require":{"laravel/framework":"^13.0"}}"#);
        host.add_file("app/Models/User.php","<?php namespace App\\Models; class User extends \\Illuminate\\Database\\Eloquent\\Model{}");
        let mut plugin=Main::default();
        plugin.activate(&host.context("aritti.laravel")).unwrap();
        plugin.advance_document_index("/project/app/Models/User.php");
        assert!(plugin.index_builders.is_empty());
        assert!(plugin.index_progress.is_empty());
        assert!(plugin.index_pending("/project"));
    }

    #[test]
    fn provider_advance_runs_no_rechecks_or_deferred_work() {
        let host=MockHost::install();
        host.add_file("composer.json",r#"{"require":{"laravel/framework":"^13.0"}}"#);
        let project=host.invocation().project().unwrap();
        let mut plugin=Main::default();
        plugin.activate(&host.context("aritti.laravel")).unwrap();
        while plugin.index_pending("/project") { plugin.advance_index(&project); }
        plugin.index_rechecks.insert("/project".into(),std::collections::VecDeque::from(["/project/composer.json".to_owned()]));
        plugin.advance_document_index("/project/composer.json");
        assert_eq!(plugin.index_rechecks["/project"].len(),1);
        plugin.advance_index(&project);
        assert!(!plugin.index_rechecks.contains_key("/project"));
    }

    #[test]
    fn activation_queues_every_scope_without_starting_a_builder() {
        let host=MockHost::install();
        host.add_file("composer.json",r#"{"require":{"laravel/framework":"^13.0"}}"#);
        host.add_project("/other");
        let mut plugin=Main::default();
        plugin.activate(&host.context("aritti.laravel")).unwrap();
        assert!(plugin.indexes.is_empty());
        assert!(plugin.index_builders.is_empty(),"activation must not parse project sources");
        assert_eq!(plugin.pending_indexes.len(),2);
        assert!(plugin.index_pending("/project"));
        assert!(plugin.completed_index_for("/project").is_none());
    }

    #[test]
    fn requeue_removes_a_snapshot_before_any_replacement_facts_are_published() {
        let host=MockHost::install();
        let project=host.invocation().project().unwrap();
        let mut plugin=Main::default();
        plugin.indexes.insert("/project".into(),Index::default());
        plugin.queue_index(&project);
        assert!(plugin.index_pending("/project"));
        assert!(plugin.completed_index_for("/project").is_none());
        assert!(plugin.indexes.is_empty());
        let view=plugin.pending_index_view(&host.invocation()).unwrap();
        let Body::Other{kind,data}=view.body else{panic!("pending body")};
        assert_eq!(kind,"message");
        let data:serde_json::Value=serde_json::from_str(&data).unwrap();
        assert_eq!(data["backgroundPoll"]["message"]["op"],"index-tick");
    }
    #[test]
    fn pending_index_ticks_cross_the_real_ui_kit_bridge_and_echo_the_poll_token() {
        let host=MockHost::install();
        host.add_file("composer.json",r#"{"require":{"laravel/framework":"^13.0"}}"#);
        let mut plugin=Main::default();
        plugin.activate(&host.context("aritti.laravel")).unwrap();
        ui_kit::web::reveal_view(VIEW,include_str!("ui_config.json")).unwrap();
        let config:serde_json::Value=serde_json::from_str(&plugin.web_config(&host.invocation())).unwrap();
        assert!(config["opaque_views"].as_array().unwrap().iter().any(|view|view==VIEW));
        let request=serde_json::json!({"channel":"ui-kit","sequence":0,"frameId":"0123456789abcdef0123456789abcdef","op":"opaque-message","project":"/project","targetView":VIEW,"message":serde_json::json!({"op":"index-tick","_backgroundPoll":"poll-1"}).to_string()});
        plugin.handle(VIEW,ViewEvent::Message(request.to_string()),&host.invocation()).unwrap();
        assert!(plugin.index_progress.get("/project").is_some_and(|(completed,_)|*completed>0));
        let replies=host.view_messages().into_iter().filter_map(|(_,raw)|serde_json::from_str::<serde_json::Value>(&raw).ok()).collect::<Vec<_>>();
        assert!(replies.iter().any(|reply|reply["channel"]=="ui-kit-view-message" && reply["targetView"]==VIEW && reply["message"].as_str().and_then(|raw|serde_json::from_str::<serde_json::Value>(raw).ok()).is_some_and(|message|message["_backgroundPoll"]=="poll-1")),"{replies:?}");
    }

    #[test]
    fn pending_index_keeps_independent_generator_controls_mounted() {
        let host=MockHost::install();
        let mut plugin=Main::default();
        plugin.activate(&host.context("aritti.laravel")).unwrap();
        let view=plugin.render(crate::generator_workspace::VIEW,None,&host.invocation()).unwrap();
        let Body::Other{kind,data}=view.body else{panic!("pollable generator HTML")};
        assert_eq!(kind,"html");
        let body:serde_json::Value=serde_json::from_str(&data).unwrap();
        assert!(body["html"].as_str().unwrap().contains("data-generator-workspace"));
        assert_eq!(body["backgroundPoll"]["message"]["op"],"index-tick");
    }


    #[test]
    fn index_ticks_advance_even_when_the_hub_envelope_targets_a_different_tab() {
        let host=MockHost::install();
        host.add_file("composer.json",r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#);
        host.add_file("artisan","<?php");
        host.add_file("resources/views/welcome.blade.php","<h1>hi</h1>");
        let mut plugin=Main::default();
        plugin.activate(&host.context("aritti.laravel")).unwrap();
        ui_kit::web::reveal_view(VIEW,include_str!("ui_config.json")).unwrap();
        let ctx=host.invocation();
        let frame="0123456789abcdef0123456789abcdef";
        let send=|plugin:&mut Main,sequence:u32,body:serde_json::Value|{
            let mut request=serde_json::json!({"channel":"ui-kit","sequence":sequence,"frameId":frame,"project":"/project","targetView":VIEW});
            request.as_object_mut().unwrap().extend(body.as_object().unwrap().clone());
            plugin.handle(VIEW,ViewEvent::Message(request.to_string()),&ctx)
        };
        send(&mut plugin,1,serde_json::json!({"op":"select-view","target":"laravel-config"})).unwrap();
        plugin.queue_indexes();
        assert!(plugin.index_work_pending("/project"));
        for tick in 0..400 {
            let message=serde_json::json!({"op":"index-tick","_backgroundPoll":format!("{frame}:{tick}")}).to_string();
            send(&mut plugin,0,serde_json::json!({"op":"opaque-message","message":message})).unwrap();
            if !plugin.index_work_pending("/project") { break; }
        }
        assert!(!plugin.index_work_pending("/project"), "index stalled: {:?}", plugin.index_progress);
        assert!(plugin.completed_index_for("/project").is_some());
    }
}
