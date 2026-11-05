//! Dependency Audit for OpenIDEA: checks dependencies against OSV.dev.

#[allow(dead_code)]
mod manifest;
#[allow(dead_code)]
mod osv;

use std::time::{SystemTime, UNIX_EPOCH};

use openidea_plugin::host;
use openidea_plugin::providers::Providers;
use openidea_plugin::views::{View, Viewport, Views};
use openidea_plugin::{Diagnostic, Error, Event, Invocation, LogLevel, Plugin, Project, register_plugin};
use spring_text::index::project_of;
use spring_text::range;

use manifest::{Dep, kind, lockfile, parse};
use osv::{Finding, audit};

#[derive(Default)]
pub struct DependencyAudit;

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

impl Plugin for DependencyAudit {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
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

impl Providers for DependencyAudit {}

impl Views for DependencyAudit {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, _ctx: &Invocation) -> Result<View, Error> {
        Ok(View::message("Run Dependency Audit: Scan Project to list vulnerable dependencies."))
    }
}

register_plugin!(DependencyAudit, providers, views);

#[cfg(test)]
mod tests;
