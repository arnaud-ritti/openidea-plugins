//! Project-owned browsing choices. Configuration values never become a runtime target.
use super::*;
use openidea_plugin::{FileKind, FileStat};
use serde_json::{Value, json};
const MAX_SOURCE: usize = 512 << 10;
const MAX_CONFIGS: usize = 64;
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: Option<u64>,
}
#[derive(Clone, Debug)]
struct Saved {
    project: String,
    root: String,
    config: Option<String>,
    root_stamp: Stamp,
    config_stamp: Option<Stamp>,
    provenance: String,
}
pub(super) enum State {
    All,
    Selected { path: String, summary: String },
    Unavailable(String),
    Invalid(String),
}
fn hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3))
}
fn key(root: &str) -> String {
    format!("browse-selection.v1.{:016x}", hash(root))
}
pub(super) fn project_root(project: &Project) -> Result<String, Error> {
    let root = project.root()?.replace('\\', "/");
    if root.len() < 2
        || root.len() > 4096
        || !(root.starts_with('/')
            || (root.as_bytes().get(1) == Some(&b':') && root.as_bytes().get(2) == Some(&b'/') && root.as_bytes()[0].is_ascii_alphabetic()))
        || root.chars().any(char::is_control)
        || root.split('/').any(|p| matches!(p, "." | ".."))
    {
        return Err(Error::Invalid("A concrete owning project is required for source selection.".into()));
    }
    Ok(root.trim_end_matches('/').into())
}
fn relative(root: &str, path: &str) -> Option<String> {
    let path = path.replace('\\', "/");
    let path = path.strip_prefix(&format!("{root}/")).unwrap_or(&path);
    if path.is_empty()
        || path.len() > 1024
        || path.starts_with('/')
        || path.contains([':', '$', '#', '{', '}', '?', '*'])
        || path.chars().any(char::is_control)
        || path.split('/').any(|p| p.is_empty() || matches!(p, "." | ".."))
    {
        return None;
    }
    Some(path.into())
}
fn stamp(project: &Project, path: &str) -> Result<Stamp, Error> {
    let FileStat { kind, size, modified } = project.stat(path)?;
    if kind != FileKind::File || size > MAX_SOURCE as u64 {
        return Err(Error::Invalid("Selected source is not a bounded project file.".into()));
    }
    Ok(Stamp { size, modified })
}
fn read(project: &Project, path: &str) -> Result<(String, Stamp), Error> {
    let metadata = stamp(project, path)?;
    let text = project.read_file(path)?;
    if text.len() > MAX_SOURCE {
        return Err(Error::Invalid("Selected source exceeds 512 KiB.".into()));
    }
    Ok((text, metadata))
}
fn decode(value: &str, root: &str) -> Result<Saved, Error> {
    if value.len() > 16384 {
        return Err(Error::Invalid("Stored source selection is oversized; clear it and choose again.".into()));
    }
    let value = crate::strict_json::parse(value, 16384)
        .map_err(|_| Error::Invalid("Stored source selection is invalid or ambiguous; no owner is inferred.".into()))?;
    if value.get("project").and_then(Value::as_str) != Some(root) {
        return Err(Error::Invalid("Stored source selection has a different project owner; refusing to reuse it.".into()));
    }
    if value.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(Error::Invalid("Stored source selection has an unsupported version; clear it and choose again.".into()));
    }
    let string = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| Error::Invalid("Stored source selection is incomplete; clear it and choose again.".into()))
    };
    let root_path = relative(root, &string("root")?).ok_or(Error::Stale)?;
    let config = match value.get("config") {
        Some(Value::Null) => None,
        Some(Value::String(path)) => Some(relative(root, path).ok_or(Error::Stale)?),
        _ => return Err(Error::Stale),
    };
    let metadata = |name: &str| -> Result<Stamp, Error> {
        let v = value.get(name).ok_or(Error::Stale)?;
        let size = v.get("size").and_then(Value::as_u64).filter(|size| *size <= MAX_SOURCE as u64).ok_or(Error::Stale)?;
        let modified = match v.get("modified") {
            Some(Value::Null) => None,
            Some(Value::Number(n)) => Some(n.as_u64().ok_or(Error::Stale)?),
            _ => return Err(Error::Stale),
        };
        Ok(Stamp { size, modified })
    };
    let root_stamp = metadata("rootStamp")?;
    let config_stamp = if config.is_some() { Some(metadata("configStamp")?) } else { None };
    let provenance = string("provenance")?;
    if provenance.len() > 2048 || provenance.chars().any(char::is_control) {
        return Err(Error::Stale);
    }
    Ok(Saved { project: root.into(), root: root_path, config, root_stamp, config_stamp, provenance })
}
// Preserve explicit recovery for broken non-JSON legacy bytes, but never let
// decoded duplicate keys or over-budget JSON erase an ambiguous source owner.
fn guard_owner(existing: Option<&str>, root: &str) -> Result<(), Error> {
    let Some(existing) = existing else { return Ok(()); };
    if existing.len() > 16384 { return Err(Error::Invalid("Stored source selection is oversized; refusing to replace an unproved owner.".into())); }
    let value = match crate::strict_json::parse(existing, 16384) {
        Ok(value) => value,
        Err(_) if serde_json::from_str::<Value>(existing).is_err() => return Ok(()),
        Err(_) => return Err(Error::Invalid("Stored source selection has ambiguous JSON; refusing to replace or delete an unproved owner.".into())),
    };
    if value.get("project").and_then(Value::as_str).is_some_and(|owner| owner != root) {
        return Err(Error::Invalid("Stored selection has a different project owner; refusing to replace or delete it.".into()));
    }
    Ok(())
}
fn encode(saved: &Saved) -> String {
    let metadata = |stamp: &Stamp| json!({"size":stamp.size,"modified":stamp.modified});
    json!({"version":1,"project":saved.project,"root":saved.root,"config":saved.config,"rootStamp":metadata(&saved.root_stamp),"configStamp":saved.config_stamp.as_ref().map(metadata),"provenance":saved.provenance}).to_string()
}
fn provenance(config: &str, text: &str, root: &str, selected: &str) -> String {
    let keys = spring_text::config::keys(config, text);
    let declarations: Vec<_> = keys
        .iter()
        .filter(|key| {
            matches!(
                key.key.as_str(),
                "changeLogFile" | "liquibase.command.changelogFile" | "liquibase.command.changeLogFile" | "spring.liquibase.change-log"
            )
        })
        .collect();
    if declarations.is_empty() {
        return "no supported changelog declaration; profile/runtime precedence is unresolved".into();
    }
    if declarations.len() != 1 {
        return "multiple source changelog declarations; effective root is unresolved".into();
    }
    let declaration = declarations[0];
    let value = declaration.value.trim().trim_matches(['\'', '"']);
    let path = value.strip_prefix("classpath:").map(|path| path.trim_start_matches('/')).unwrap_or(value);
    let matches = relative(root, path).is_some_and(|path| {
        [path.clone(), format!("src/main/resources/{path}"), format!("src/test/resources/{path}")]
            .iter()
            .any(|candidate| candidate == selected)
    });
    let status = if value.contains(['$', '#', '{', '}']) || value.contains("://") || relative(root, path).is_none() {
        "dynamic/unresolved declaration"
    } else if matches {
        "matches the selected source root"
    } else {
        "does not establish the selected source root"
    };
    format!(
        "{}:{} {}: {}; profile/runtime precedence is unresolved",
        config,
        openidea_plugin::text::position(text, declaration.at).line + 1,
        declaration.key,
        status
    )
}
fn config_files(project: &Project, root: &str) -> (Vec<String>, bool) {
    let mut files = BTreeSet::new();
    let mut partial = false;
    let deadline = Instant::now() + DISCOVERY_BUDGET;
    for glob in ["**/*liquibase*.properties", "**/application*.properties", "**/application*.yaml", "**/application*.yml"] {
        if Instant::now() > deadline {
            partial = true;
            break;
        }
        match project.find_files(glob, MAX_CONFIGS as u32) {
            Ok(found) => {
                partial |= found.len() >= MAX_CONFIGS;
                for path in found {
                    if let Some(path) = relative(root, &path) {
                        files.insert(path);
                    }
                }
            }
            Err(_) => partial = true,
        }
    }
    partial |= files.len() > MAX_CONFIGS;
    (files.into_iter().take(MAX_CONFIGS).collect(), partial)
}
pub(super) fn browse_target(project: &Project, root: &str, changelog: &str, include: &Include) -> Target {
    let candidates = changelog::candidates(changelog, root, include);
    if include.path.is_empty() || candidates.is_empty() {
        return Target::Unknown;
    }
    let mut unknown = false;
    for candidate in candidates {
        match project.stat(&candidate) {
            Ok(metadata) if (include.dir && metadata.kind == FileKind::Directory) || (!include.dir && metadata.kind == FileKind::File) => {
                return Target::Found(candidate);
            }
            Ok(metadata) if matches!(metadata.kind, FileKind::Symlink | FileKind::Other) => unknown = true,
            Ok(_) | Err(Error::Invalid(_) | Error::NotFound(_)) => {}
            Err(_) => unknown = true,
        }
    }
    if unknown { Target::Unknown } else { Target::Missing }
}

impl Liquibase {
    pub(super) fn selection_state(&self, project: &Project) -> State {
        let Ok(root) = project_root(project) else {
            return State::Invalid("A concrete owning project is required for source selection.".into());
        };
        let value = match host::storage::get(&key(&root)) {
            Ok(None) => return State::All,
            Ok(Some(value)) => value,
            Err(_) => return State::Unavailable("Source selection storage is unavailable; project browsing is unfiltered.".into()),
        };
        let saved = match decode(&value, &root) {
            Ok(saved) => saved,
            Err(_) => {
                return State::Invalid(
                    "Stored project source selection is invalid or has another owner. Clear or reselect it explicitly.".into(),
                );
            }
        };
        let path = format!("{root}/{}", saved.root);
        let (text, root_stamp) = match read(project, &path) {
            Ok(source) => source,
            Err(_) => {
                return State::Invalid(format!(
                    "Selected changelog {} is missing, unreadable or oversized. Clear or reselect it explicitly.",
                    saved.root
                ));
            }
        };
        if !changelog::parse(&path, &text).is_some_and(|log| log.header) {
            return State::Invalid(format!(
                "Selected source {} is no longer a recognized changelog. Clear or reselect it explicitly.",
                saved.root
            ));
        }
        let mut changed = root_stamp != saved.root_stamp;
        let mut unknown = root_stamp.modified.is_none();
        let summary = if let Some(config) = &saved.config {
            let (text, metadata) = match read(project, &format!("{root}/{config}")) {
                Ok(source) => source,
                Err(_) => {
                    return State::Invalid(format!(
                        "Selected source configuration {config} is missing, unreadable or oversized. Clear or reselect it explicitly."
                    ));
                }
            };
            let current = provenance(config, &text, &root, &saved.root);
            changed |= Some(&metadata) != saved.config_stamp.as_ref() || current != saved.provenance;
            unknown |= metadata.modified.is_none();
            format!("root {}; config {config}; {current}", saved.root)
        } else {
            format!("root {}; no configuration selected", saved.root)
        };
        let freshness = if changed {
            "source selection changed since it was recorded; reselect to acknowledge"
        } else if unknown {
            "modification time unavailable; freshness is unverified"
        } else {
            "source metadata matches the recorded selection"
        };
        State::Selected {
            path,
            summary: format!("Browser source selection: {summary}; {freshness}. Effective database/runtime target is unresolved."),
        }
    }
    pub(super) fn choose_sources(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or_else(|| Error::Invalid("Open the owning project to choose Liquibase source files.".into()))?;
        let root = project_root(&project)?;
        // Persist only after all prompts and fresh source reads succeed.
        let existing = host::storage::get(&key(&root))?;
        guard_owner(existing.as_deref(), &root)?;
        let _task = host::begin_task("Reading Liquibase changelogs", false);
        let mut walk = Walk::new(&project, &root, self.max_files.unwrap_or(MAX_FILES));
        walk.discover();
        let paths: Vec<_> = walk.found.iter().filter(|(_, log)| log.1.header).filter_map(|(path, _)| relative(&root, path)).collect();
        if paths.is_empty() {
            return Err(Error::NotFound("No bounded Liquibase source changelogs were discovered.".into()));
        }
        let items: Vec<_> =
            paths.iter().map(|path| PickItem::new(path, path).description("Project changelog source; browsing selection only")).collect();
        let title = if walk.partial { "Choose browser changelog root (partial discovery)" } else { "Choose browser changelog root" };
        let Some(chosen) = host::pick(title, &items)? else {
            return Ok(None);
        };
        if !paths.contains(&chosen) {
            return Err(Error::Stale);
        }
        let (files, partial) = config_files(&project, &root);
        let mut items = vec![PickItem::new("none", "No configuration source")];
        items
            .extend(files.iter().map(|path| PickItem::new(path, path).description("Source provenance only; connection values are hidden")));
        let Some(config) =
            host::pick(if partial { "Choose configuration source (partial discovery)" } else { "Choose configuration source" }, &items)?
        else {
            return Ok(None);
        };
        let config = if config == "none" {
            None
        } else if files.contains(&config) {
            Some(config)
        } else {
            return Err(Error::Stale);
        };
        let absolute = format!("{root}/{chosen}");
        let (text, root_stamp) = read(&project, &absolute)?;
        if !changelog::parse(&absolute, &text).is_some_and(|log| log.header) {
            return Err(Error::Stale);
        }
        let (config_stamp, provenance) = if let Some(config) = &config {
            let (text, stamp) = read(&project, &format!("{root}/{config}"))?;
            (Some(stamp), provenance(config, &text, &root, &chosen))
        } else {
            (None, "no configuration selected".into())
        };
        let saved = Saved { project: root.clone(), root: chosen, config, root_stamp, config_stamp, provenance };
        // Storage has no compare-and-set; detect changes during prompts without claiming atomicity.
        if host::storage::get(&key(&root))? != existing {
            return Err(Error::Stale);
        }
        let encoded = encode(&saved);
        if existing.as_deref() == Some(encoded.as_str()) {
            return Ok(Some("Source browsing selection is unchanged.".into()));
        }
        host::storage::set(&key(&root), &encoded)?;
        self.targets.clear();
        self.targets_root = None;
        crate::refresh_browser();
        Ok(Some("Saved this project's source browsing selection. CLI runtime configuration is resolved separately.".into()))
    }
    pub(super) fn clear_sources(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or_else(|| Error::Invalid("Open the owning project to clear its source selection.".into()))?;
        let root = project_root(&project)?;
        let existing = host::storage::get(&key(&root))?;
        guard_owner(existing.as_deref(), &root)?;
        if existing.is_none() { return Ok(None); }
        host::storage::delete(&key(&root))?;
        self.targets.clear();
        self.targets_root = None;
        crate::refresh_browser();
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    fn xml(id: &str) -> String {
        format!(
            r#"<databaseChangeLog xmlns="http://www.liquibase.org/xml/ns/dbchangelog"><changeSet id="{id}" author="author"/></databaseChangeLog>"#
        )
    }
    fn setup() -> MockHost {
        let host = MockHost::install();
        host.add_file("/project/a.xml", &xml("a"));
        host.add_file("/project/b.xml", &xml("b"));
        host.add_file(
            "/project/liquibase.properties",
            "changeLogFile=a.xml\nurl=jdbc:private:secret-host\nusername=private-user\npassword=VERY_PRIVATE_PASSWORD\n",
        );
        host
    }
    fn choose(host: &MockHost, plugin: &mut Liquibase, root: &str, config: &str) {
        host.answer_next_prompt(Some(&[root]));
        host.answer_next_prompt(Some(&[config]));
        plugin.choose_sources(&host.invocation()).unwrap();
    }
    fn state(plugin: &Liquibase, host: &MockHost) -> State {
        plugin.selection_state(&host.invocation().project().unwrap())
    }
    fn labels(view: &View) -> Vec<&str> {
        match &view.body {
            Body::Tree(items) => items.iter().map(|item| item.label.as_str()).collect(),
            _ => Vec::new(),
        }
    }
    #[test]
    fn external_selection_and_clear_notify_only_browser_without_source_values() {
        let host=setup();let mut plugin=Liquibase::default();
        choose(&host,&mut plugin,"a.xml","liquibase.properties");
        assert_eq!(host.refreshed_views(),["changesets"]);
        assert!(host.view_messages().iter().all(|(_,text)| serde_json::from_str::<serde_json::Value>(text).ok().is_some_and(|value| value["channel"] == "ui-kit-refresh" && value.as_object().unwrap().len() <= 2)));
        plugin.clear_sources(&host.invocation()).unwrap();
        assert_eq!(host.refreshed_views(),["changesets","changesets"]);
        assert!(host.view_messages().iter().all(|(_,text)| serde_json::from_str::<serde_json::Value>(text).ok().is_some_and(|value| value["channel"] == "ui-kit-refresh" && value.as_object().unwrap().len() <= 2)));
        assert!(host.requests().is_empty());assert!(host.process_runs().is_empty());
    }
    #[test]
    fn cancelled_identical_or_absent_selection_emits_no_false_change() {
        let host=setup();let mut plugin=Liquibase::default();
        host.answer_next_prompt(None);plugin.choose_sources(&host.invocation()).unwrap();
        plugin.clear_sources(&host.invocation()).unwrap();assert!(host.refreshed_views().is_empty());
        choose(&host,&mut plugin,"a.xml","none");let before=host.refreshed_views().len();
        choose(&host,&mut plugin,"a.xml","none");assert_eq!(host.refreshed_views().len(),before);
    }
    #[test]
    fn foreign_missing_and_revoked_owner_get_safe_replacement_model() {
        let host=setup();let mut plugin=Liquibase::default();choose(&host,&mut plugin,"a.xml","none");
        let first=plugin.render("changesets",None,&host.invocation()).unwrap();assert!(labels(&first).contains(&"a.xml"));
        host.set_project_root(Some("/other"));host.add_file("/other/c.xml",&xml("c"));
        let foreign=plugin.render("changesets",None,&host.invocation()).unwrap();assert!(!labels(&foreign).contains(&"a.xml"));
        assert!(labels(&foreign).contains(&"c.xml"));
        host.set_project_root(None);assert!(matches!(plugin.render("changesets",None,&host.invocation()).unwrap().body,Body::Message(_)));
        host.set_project_root(Some("/other"));let host=host.with_capabilities(&["storage"]);
        assert!(matches!(plugin.render("changesets",None,&host.invocation()).unwrap().body,Body::Message(text) if text.contains("The project folder could not be read")));
    }
    #[test]
    fn native_render_needs_no_web_bridge_or_refresh_feedback() {
        let host=setup();let mut plugin=Liquibase::default();choose(&host,&mut plugin,"a.xml","none");
        let before=host.refreshed_views().len();
        for _ in 0..2 {
            let rendered=plugin.render("changesets",None,&host.invocation()).unwrap();
            assert!(matches!(rendered.body,Body::Tree(_)));
            assert!(rendered.toolbar.iter().any(|button|button.id=="browser-filter"));
        }
        assert_eq!(host.refreshed_views().len(),before);
        assert!(host.view_messages().iter().all(|(_,text)| serde_json::from_str::<serde_json::Value>(text).ok().is_some_and(|value| value["channel"] == "ui-kit-refresh" && value.as_object().unwrap().len() <= 2)));
    }
    #[test]
    fn foreign_browser_render_invalidates_old_navigation_without_opening_source() {
        let host=setup();let mut plugin=Liquibase::default();let view=plugin.render("changesets",None,&host.invocation()).unwrap();
        let Body::Tree(rows)=view.body else {panic!("tree expected")};let key=rows.iter().find(|r|r.key.contains("/c")).unwrap().key.clone();
        host.set_project_root(Some("/other"));plugin.render("changesets",None,&host.invocation()).unwrap();
        assert!(matches!(plugin.handle("changesets",ViewEvent::Activate(key),&host.invocation()),Err(Error::Stale)));
        assert!(host.opened_locations().is_empty());
    }

    #[test]
    fn persists_explicit_source_root_and_config_without_secret_values_or_cli_execution() {
        let host = setup();
        let mut plugin = Liquibase::default();
        choose(&host, &mut plugin, "a.xml", "liquibase.properties");
        let storage = host.storage();
        let value = storage.get(&key("/project")).unwrap();
        for secret in ["secret-host", "private-user", "VERY_PRIVATE_PASSWORD", "jdbc:"] {
            assert!(!value.contains(secret));
        }
        let mut reloaded = Liquibase::default();
        let view = reloaded.render("changesets", None, &host.invocation()).unwrap();
        let labels = labels(&view);
        assert!(labels.contains(&"a.xml"));
        assert!(!labels.contains(&"b.xml"));
        let status = view.status.unwrap();
        assert!(status.contains("liquibase.properties:1 changeLogFile"));
        assert!(status.contains("matches the selected source root"));
        assert!(status.contains("Effective database/runtime target is unresolved"));
        assert!(status.contains("freshness is unverified"));
        assert!(host.terminals().is_empty());
    }
    #[test]
    fn cancellation_and_forged_choices_never_persist() {
        let host = setup();
        let mut plugin = Liquibase::default();
        host.answer_next_prompt(None);
        plugin.choose_sources(&host.invocation()).unwrap();
        assert!(host.storage().is_empty());
        host.answer_next_prompt(Some(&["a.xml"]));
        host.answer_next_prompt(None);
        plugin.choose_sources(&host.invocation()).unwrap();
        assert!(host.storage().is_empty());
        host.answer_next_prompt(Some(&["../other.xml"]));
        assert!(plugin.choose_sources(&host.invocation()).is_err());
        assert!(host.storage().is_empty());
    }
    #[test]
    fn choices_are_project_owned_and_anonymous_context_cannot_save_or_clear() {
        let host = setup();
        host.add_file("/other/b.xml", &xml("other"));
        let mut plugin = Liquibase::default();
        choose(&host, &mut plugin, "a.xml", "none");
        host.set_project_root(Some("/other"));
        assert!(matches!(state(&plugin, &host), State::All));
        choose(&host, &mut plugin, "b.xml", "none");
        assert_eq!(host.storage().len(), 2);
        host.set_project_root(Some("/project"));
        assert!(matches!(state(&plugin,&host),State::Selected {path,..} if path=="/project/a.xml"));
        let before = host.storage();
        host.set_project_root(None);
        assert!(plugin.choose_sources(&host.invocation()).is_err());
        assert!(plugin.clear_sources(&host.invocation()).is_err());
        assert_eq!(host.storage(), before);
    }
    #[test]
    fn changed_provenance_and_missing_config_are_explicit_and_do_not_silently_reset() {
        let host = setup();
        let mut plugin = Liquibase::default();
        choose(&host, &mut plugin, "a.xml", "liquibase.properties");
        host.add_file("/project/liquibase.properties", "changeLogFile=b.xml\npassword=DO_NOT_SHOW\n");
        let State::Selected { summary, .. } = state(&plugin, &host) else { panic!("selection must remain explicit") };
        assert!(summary.contains("changed since"));
        assert!(!summary.contains("DO_NOT_SHOW"));
        let mut record: Value = serde_json::from_str(&host.storage()[&key("/project")]).unwrap();
        record["config"] = json!("missing.properties");
        host::storage::set(&key("/project"), &record.to_string()).unwrap();
        let view = plugin.render("changesets", None, &host.invocation()).unwrap();
        assert!(matches!(view.body,Body::Message(message) if message.contains("missing")));
        assert!(plugin.targets.is_empty());
        assert!(host.storage().contains_key(&key("/project")));
        plugin.clear_sources(&host.invocation()).unwrap();
        assert!(matches!(state(&plugin, &host), State::All));
    }
    #[test]
    fn ambiguous_decoded_project_owner_is_not_adopted_replaced_or_cleared() {
        let host=setup();let mut plugin=Liquibase::default();choose(&host,&mut plugin,"a.xml","none");
        let original=host.storage()[&key("/project")].clone();
        let raw=original.replace("\"project\":\"/project\"",r#""project":"/other","pro\u006aect":"/project""#);
        assert_ne!(raw,original);host::storage::set(&key("/project"),&raw).unwrap();
        let view=plugin.render("changesets",None,&host.invocation()).unwrap();assert!(matches!(view.body,Body::Message(_)));assert!(plugin.targets.is_empty());
        assert!(plugin.run_command("clear-sources",&host.invocation(),None).is_err());
        assert_eq!(host.storage()[&key("/project")],raw);
        assert!(plugin.run_command("choose-sources",&host.invocation(),None).is_err());
        assert_eq!(host.storage()[&key("/project")],raw);
    }
    #[test]
    fn corrupt_data_recovers_explicitly_and_foreign_owner_is_not_overwritten_or_deleted() {
        let host = setup();
        let mut plugin = Liquibase::default();
        host::storage::set(&key("/project"), "broken").unwrap();
        assert!(matches!(state(&plugin, &host), State::Invalid(_)));
        plugin.clear_sources(&host.invocation()).unwrap();
        assert!(host.storage().is_empty());
        host::storage::set(&key("/project"), r#"{"version":1,"project":"/other"}"#).unwrap();
        let before = host.storage();
        assert!(matches!(state(&plugin, &host), State::Invalid(_)));
        assert!(plugin.choose_sources(&host.invocation()).is_err());
        assert!(plugin.clear_sources(&host.invocation()).is_err());
        assert_eq!(host.storage(), before);
    }
    #[test]
    fn source_paths_are_bounded_and_cannot_escape_project() {
        for input in ["../a.xml", "/other/a.xml", "a/../b.xml", "https://host/a.xml", "${root}/a.xml", "a//b.xml", "a\nb.xml"] {
            assert!(relative("/project", input).is_none(), "{input}");
        }
        assert_eq!(relative("/project", "/project/path with spaces/a.xml").as_deref(), Some("path with spaces/a.xml"));
        assert_eq!(relative("C:/project", "C:/project/a.xml").as_deref(), Some("a.xml"));
        assert!(relative("/project", &"a".repeat(1025)).is_none());
    }
    #[test]
    fn source_provenance_only_exposes_supported_keys_and_handles_dynamic_or_duplicate_roots() {
        let dynamic = provenance("liquibase.properties", "changeLogFile=${PRIVATE_PATH}\npassword=PRIVATE_SECRET\n", "/project", "a.xml");
        assert!(dynamic.contains("dynamic/unresolved"));
        assert!(!dynamic.contains("PRIVATE"));
        let multiple = provenance(
            "application.yaml",
            "spring:\n  liquibase:\n    change-log: classpath:/a.xml\n---\nspring:\n  liquibase:\n    change-log: classpath:/b.xml\n",
            "/project",
            "a.xml",
        );
        assert!(multiple.contains("multiple"));
        assert!(
            provenance(
                "application.properties",
                "spring.liquibase.change-log=classpath:/db/a.xml\n",
                "/project",
                "src/main/resources/db/a.xml"
            )
            .contains("matches the selected")
        );
        assert!(provenance("liquibase.properties", "changeLogFile=/project/a.xml\n", "/project", "a.xml").contains("matches the selected"));
    }
    #[test]
    fn selected_root_is_retained_when_bounded_discovery_is_partial_and_old_targets_cannot_cross_project() {
        let host = setup();
        let mut plugin = Liquibase::default();
        choose(&host, &mut plugin, "b.xml", "none");
        host.add_file("/project/b.xml", &xml("b").replace("</databaseChangeLog>", r#"<include file="a.xml"/></databaseChangeLog>"#));
        plugin.max_files = Some(1);
        let view = plugin.render("changesets", None, &host.invocation()).unwrap();
        assert!(labels(&view).contains(&"b.xml"));
        assert!(view.status.unwrap().contains("partial"));
        let key = plugin.targets.keys().next().unwrap().clone();
        host.set_project_root(Some("/other"));
        assert!(matches!(plugin.handle("changesets", ViewEvent::Activate(key), &host.invocation()), Err(Error::Stale)));
    }
    #[test]
    fn storage_denial_leaves_normal_browsing_and_does_not_create_anonymous_config() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        host.add_file("a.xml", &xml("a"));
        let mut plugin = Liquibase::default();
        assert!(matches!(state(&plugin, &host), State::Unavailable(_)));
        let view = plugin.render("changesets", None, &host.invocation()).unwrap();
        assert!(!labels(&view).is_empty());
        assert!(plugin.choose_sources(&host.invocation()).is_err());
        assert!(host.storage().is_empty());
    }
    #[test]
    fn root_changes_are_visible_and_missing_or_unrecognized_roots_block_selected_browsing() {
        let host = setup();
        let mut plugin = Liquibase::default();
        choose(&host, &mut plugin, "a.xml", "none");
        host.add_file("/project/a.xml", &xml("changed-id"));
        let State::Selected { summary, .. } = state(&plugin, &host) else { panic!("expected selected root") };
        assert!(summary.contains("changed since"));
        host.add_file("/project/a.xml", "not a changelog");
        assert!(matches!(state(&plugin,&host),State::Invalid(message) if message.contains("no longer")));
        let mut record: Value = serde_json::from_str(&host.storage()[&key("/project")]).unwrap();
        record["root"] = json!("missing.xml");
        host::storage::set(&key("/project"), &record.to_string()).unwrap();
        assert!(matches!(state(&plugin,&host),State::Invalid(message) if message.contains("missing")));
        assert!(matches!(plugin.render("changesets", None, &host.invocation()).unwrap().body, Body::Message(_)));
        assert!(plugin.targets.is_empty());
    }
    #[test]
    fn browser_source_buttons_keep_project_bound_bridge_routing() {
        let config: Value = serde_json::from_str(include_str!("ui_config.json")).unwrap();
        let buttons = config["project_bound_buttons"].as_array().unwrap();
        for key in ["choose-sources", "clear-sources"] {
            assert!(
                buttons.iter().any(|value| value.as_str() == Some(key)),
                "{key} must retain its project guard when UI assets are synchronized"
            );
        }
    }
    #[test]
    fn selected_root_without_include_all_does_not_scan_unrelated_catalog_files() {
        let host = setup();
        let mut plugin = Liquibase::default();
        choose(&host, &mut plugin, "a.xml", "none");
        host.add_file("/project/b.xml", &" ".repeat(MAX_SOURCE + 1));
        plugin.max_files = Some(1);
        let view = plugin.render("changesets", None, &host.invocation()).unwrap();
        assert!(labels(&view).contains(&"a.xml"));
        assert!(!view.status.unwrap().contains("partial"));
    }
    #[test]
    fn selected_include_all_still_uses_bounded_catalog_members() {
        let host = setup();
        host.add_dir("/project/dir");
        host.add_file("/project/dir/child.xml", &xml("child"));
        host.add_file(
            "/project/a.xml",
            &xml("a").replace("</databaseChangeLog>", r#"<includeAll path="dir" relativeToChangelogFile="true"/></databaseChangeLog>"#),
        );
        let mut plugin = Liquibase::default();
        choose(&host, &mut plugin, "a.xml", "none");
        let view = plugin.render("changesets", None, &host.invocation()).unwrap();
        let labels = labels(&view);
        assert!(labels.contains(&"a.xml"));
        assert!(labels.contains(&"dir/child.xml"));
        assert!(!labels.contains(&"b.xml"));
    }
}
