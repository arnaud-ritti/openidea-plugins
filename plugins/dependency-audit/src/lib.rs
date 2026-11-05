//! Dependency Audit for OpenIDEA: checks dependencies against OSV.dev.

mod manifest;
mod osv;

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use openidea_plugin::host;
use openidea_plugin::providers::{CodeAction, Document, Providers};
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Diagnostic, Error, Event, Invocation, LogLevel, NotificationLevel, Plugin, Project, Range, Severity, TextEdit, register_plugin};
use spring_text::index::project_of;
use serde::{Deserialize, Serialize};
use spring_text::range;
use spring_text::tree::Expansion;

use manifest::{Dep, GLOBS, bumped, kind, lockfile, parse};
use osv::{Finding, audit, cached_advisory, compare, level};

#[derive(Default)]
pub struct DependencyAudit {
    expansion: Expansion,
    targets: HashMap<String, Target>,
}

enum Target {
    Location(String, u32),
    Url(String),
}

#[derive(Serialize, Deserialize)]
struct Row {
    path: String,
    line: u32,
    name: String,
    version: String,
    fix: Option<String>,
    advisories: Vec<String>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
}

fn read_deps(project: &Project, path: &str) -> Result<(String, Vec<Dep>), Error> {
    let text = project.read_file(path)?;
    let lock = lockfile(path).and_then(|lock| project.read_file(&lock).ok());
    let deps = parse(path, &text, lock.as_deref());

    Ok((text, deps))
}

fn diagnostic(finding: &Finding, text: &str) -> Diagnostic {
    Diagnostic {
        range: range(text, finding.dep.at.0, finding.dep.at.1),
        severity: finding.severity(),
        message: finding.message(),
        code: finding.advisories.first().map(|a| a.id.clone()),
        source: Some("OSV".into()),
        unnecessary: false,
        deprecated: false,
        related: Vec::new(),
    }
}

fn check(path: &str) -> Result<(), Error> {
    let project = project_of(path).ok_or_else(|| Error::NotFound(format!("project of {path}")))?;
    let (text, deps) = read_deps(&project, path)?;
    let _task = host::begin_task("Checking dependencies with OSV.dev", false);
    let findings = audit(&deps, true, now())?;
    let items: Vec<Diagnostic> = findings.iter().map(|f| diagnostic(f, &text)).collect();

    host::publish_diagnostics(path, &items)
}

fn rows_key(project: &Project) -> Result<String, Error> {
    Ok(format!("scan:{}", project.root()?))
}

fn stored_rows(project: &Project) -> Result<Option<Vec<Row>>, Error> {
    let stored = host::storage::get(&rows_key(project)?)?;

    Ok(stored.and_then(|json| serde_json::from_str(&json).ok()))
}

fn scan(project: &Project) -> Result<String, Error> {
    let task = host::begin_task("Scanning dependencies", true);
    let mut files: Vec<String> = Vec::new();
    for glob in GLOBS {
        for path in project.find_files(glob, 200)? {
            if kind(&path).is_some() && !files.contains(&path) {
                files.push(path);
            }
        }
    }

    let mut texts: HashMap<String, String> = HashMap::new();
    let mut file_deps: Vec<(String, Vec<Dep>)> = Vec::new();
    let mut deps: Vec<Dep> = Vec::new();
    let mut seen = HashSet::new();
    for path in &files {
        if task.is_cancelled() {
            return Err(Error::Cancelled);
        }

        let (text, found) = match read_deps(project, path) {
            Ok(read) => read,
            Err(e) => {
                host::log(LogLevel::Warn, &format!("Scan skipped {path}: {e}"));
                continue;
            }
        };
        for dep in &found {
            if seen.insert(dep_key(dep)) {
                deps.push(dep.clone());
            }
        }
        texts.insert(path.clone(), text);
        file_deps.push((path.clone(), found));
    }

    let findings = audit(&deps, true, now())?;
    let by_dep: HashMap<_, &Finding> = findings.iter().map(|f| (dep_key(&f.dep), f)).collect();
    for (path, found) in &file_deps {
        let text = &texts[path];
        let items: Vec<Diagnostic> = found
            .iter()
            .filter_map(|dep| by_dep.get(&dep_key(dep)).map(|f| Finding { dep: dep.clone(), advisories: f.advisories.clone() }))
            .map(|f| diagnostic(&f, text))
            .collect();
        if let Err(e) = host::publish_diagnostics(path, &items) {
            host::log(LogLevel::Warn, &format!("Diagnostics of {path}: {e}"));
        }
    }

    let rows: Vec<Row> = findings
        .iter()
        .map(|f| Row {
            path: f.dep.path.clone(),
            line: texts.get(&f.dep.path).map(|text| range(text, f.dep.at.0, f.dep.at.1).start.line).unwrap_or_default(),
            name: f.dep.name.clone(),
            version: f.dep.version.clone(),
            fix: f.fix(),
            advisories: f.advisories.iter().map(|a| a.id.clone()).collect(),
        })
        .collect();
    let stored = serde_json::to_string(&rows).map_err(|e| Error::Failed(e.to_string()))?;
    let key = rows_key(project)?;
    let kept = host::storage::set(&key, &stored);
    if let Err(e) = &kept {
        let _ = host::storage::delete(&key);
        host::log(LogLevel::Warn, &format!("Scan results not stored: {e}"));
    }
    host::refresh_view("vulnerabilities");

    let mut body = match rows.is_empty() {
        true => format!("No known vulnerabilities in {} dependencies ({} files).", deps.len(), texts.len()),
        false => format!(
            "Vulnerable dependencies: {} of {} ({} files). See the Vulnerable Dependencies tool window.",
            rows.len(),
            deps.len(),
            texts.len()
        ),
    };
    if kept.is_err() {
        body.push_str(" (Too many results to keep for the tool window.)");
    }

    Ok(body)
}

fn dep_key(dep: &Dep) -> (manifest::Ecosystem, String, String) {
    (dep.ecosystem, dep.name.clone(), dep.version.clone())
}

fn tree_of(root: &str, rows: &[Row]) -> (Vec<TreeItem>, HashMap<String, Target>) {
    let root = format!("{root}/");
    let mut items: Vec<TreeItem> = Vec::new();
    let mut targets = HashMap::new();
    for row in rows {
        if !targets.contains_key(&row.path) {
            let label = row.path.strip_prefix(&root).unwrap_or(&row.path);
            items.push(TreeItem::new(&row.path, label).icon("file"));
            targets.insert(row.path.clone(), Target::Location(row.path.clone(), 0));
        }

        let key = format!("{}|{} {}", row.path, row.name, row.version);
        let detail = row.fix.as_ref().map_or("no fix".to_owned(), |fix| format!("fix: {fix}"));
        items.push(TreeItem::new(&key, &format!("{} {}", row.name, row.version)).parent(&row.path).icon("package").detail(&detail));
        targets.insert(key.clone(), Target::Location(row.path.clone(), row.line));

        for id in &row.advisories {
            let advisory_key = format!("{key}|{id}");
            let item = TreeItem::new(&advisory_key, id).parent(&key);
            items.push(match cached_advisory(id) {
                Some(advisory) => {
                    let icon = match level(advisory.severity.as_deref()) {
                        Severity::Error => "error",
                        Severity::Warning => "warning",
                        _ => "info",
                    };
                    item.icon(icon).detail(&advisory.summary)
                }
                None => item.icon("warning"),
            });
            targets.insert(advisory_key, Target::Url(format!("https://osv.dev/vulnerability/{id}")));
        }
    }

    (items, targets)
}

fn scan_and_notify(project: &Project) -> Result<(), Error> {
    let body = scan(project)?;
    host::notify(NotificationLevel::Info, "Dependency Audit", &body);

    Ok(())
}

fn scan_button(label: &str) -> Button {
    Button::new("scan", label).icon("refresh")
}

fn is_single_constraint(spec: &str) -> bool {
    let version = spec.trim_start_matches(['^', '~', '=', '>', '<', 'v']);

    version.starts_with(|c: char| c.is_ascii_digit()) && !version.contains([' ', '|', ',', '*'])
}

fn overlaps(dep: &Dep, text: &str, selection: &Range) -> bool {
    let underlined = range(text, dep.at.0, dep.at.1);

    underlined.end.line >= selection.start.line && underlined.start.line <= selection.end.line
}

fn upgrade_actions(findings: &[Finding], doc: &Document) -> Vec<CodeAction> {
    // Deps sharing a `${property}` share one range: one action, the highest fix.
    let mut upgrades: Vec<((usize, usize), String, String)> = Vec::new();

    for finding in findings {
        let Some(at) = finding.dep.version_at else {
            continue;
        };
        let Some(fix) = finding.fix() else {
            continue;
        };
        if !doc.text.get(at.0..at.1).is_some_and(is_single_constraint) {
            continue;
        }

        match upgrades.iter_mut().find(|(range_at, ..)| *range_at == at) {
            Some((_, best, _)) => {
                if compare(&fix, best) == Ordering::Greater {
                    *best = fix;
                }
            }
            None => {
                let short = finding.dep.name.rsplit(':').next().unwrap_or(&finding.dep.name);
                upgrades.push((at, fix, short.to_owned()));
            }
        }
    }

    upgrades
        .into_iter()
        .map(|((from, to), fix, short)| CodeAction {
            title: format!("Upgrade {short} to {fix}"),
            kind: Some("quickfix".into()),
            preferred: true,
            edits: vec![(
                doc.path.clone(),
                vec![TextEdit { range: range(&doc.text, from, to), text: bumped(&doc.text[from..to], &fix) }],
            )],
            data: None,
        })
        .collect()
}

impl Plugin for DependencyAudit {
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        if id != "scan" {
            return Err(Error::NotFound(format!("command {id}")));
        }

        let project = ctx.project().ok_or("Open a project first.")?;
        scan_and_notify(&project)?;

        Ok(None)
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        if host::setting_bool("autoCheck") == Some(false) {
            return;
        }

        let mut paths: Vec<&String> = Vec::new();
        for event in events {
            let (Event::FileOpened(path) | Event::FileSaved(path)) = event else { continue };
            if kind(path).is_some() && !paths.contains(&path) {
                paths.push(path);
            }
        }

        for path in paths {
            if let Err(e) = check(path) {
                host::log(LogLevel::Warn, &format!("OSV check of {path}: {e}"));
            }
        }
    }
}

impl Providers for DependencyAudit {
    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        if kind(&doc.path).is_none() {
            return Vec::new();
        }

        let Some(project) = project_of(&doc.path) else {
            return Vec::new();
        };
        let lock = lockfile(&doc.path).and_then(|lock| project.read_file(&lock).ok());
        let deps: Vec<Dep> = parse(&doc.path, &doc.text, lock.as_deref())
            .into_iter()
            .filter(|dep| overlaps(dep, &doc.text, &selection))
            .collect();
        if deps.is_empty() {
            return Vec::new();
        }

        let Ok(findings) = audit(&deps, false, now()) else {
            return Vec::new();
        };

        upgrade_actions(&findings, doc)
    }
}

impl DependencyAudit {
    fn rebuild_targets(&mut self, ctx: &Invocation) -> Result<(), Error> {
        let Some(project) = ctx.project() else {
            return Ok(());
        };
        let Some(rows) = stored_rows(&project)? else {
            return Ok(());
        };

        self.targets = tree_of(&project.root()?, &rows).1;

        Ok(())
    }
}

impl Views for DependencyAudit {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        let Some(project) = ctx.project() else {
            return Ok(View::message("Open a project first."));
        };
        let Some(rows) = stored_rows(&project)? else {
            return Ok(View::message("Run Dependency Audit: Scan Project to check this project's dependencies against OSV.dev.").button(Button::new("scan", "Scan")));
        };
        if rows.is_empty() {
            return Ok(View::message("No known vulnerabilities found.").button(Button::new("scan", "Rescan")));
        }

        let (mut items, targets) = tree_of(&project.root()?, &rows);
        self.targets = targets;
        self.expansion.apply(&mut items);

        Ok(View::new(Body::Tree(items)).button(scan_button("Rescan")).status(&format!("{} vulnerable dependencies", rows.len())))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if self.expansion.handle(&event) {
            host::refresh_view(view);

            return Ok(());
        }

        match event {
            ViewEvent::Button(id) if id == "scan" => {
                let project = ctx.project().ok_or("Open a project first.")?;
                scan_and_notify(&project)
            }
            ViewEvent::Activate(key) => {
                if !self.targets.contains_key(&key) {
                    self.rebuild_targets(ctx)?;
                }

                match self.targets.get(&key) {
                    Some(Target::Location(path, line)) => host::open_location(path, *line, 0),
                    Some(Target::Url(url)) => host::open_url(url),
                    None => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }
}

register_plugin!(DependencyAudit, providers, views);

#[cfg(test)]
mod tests;
