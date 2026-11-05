//! Dependency Audit for OpenIDEA: checks dependencies against OSV.dev.

mod manifest;
mod osv;

use std::cmp::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use openidea_plugin::host;
use openidea_plugin::providers::{CodeAction, Document, Providers};
use openidea_plugin::views::{View, Viewport, Views};
use openidea_plugin::{Diagnostic, Error, Event, Invocation, LogLevel, Plugin, Project, Range, TextEdit, register_plugin};
use spring_text::index::project_of;
use spring_text::range;

use manifest::{Dep, bumped, kind, lockfile, parse};
use osv::{Finding, audit, compare};

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

impl Views for DependencyAudit {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, _ctx: &Invocation) -> Result<View, Error> {
        Ok(View::message("Run Dependency Audit: Scan Project to list vulnerable dependencies."))
    }
}

register_plugin!(DependencyAudit, providers, views);

#[cfg(test)]
mod tests;
