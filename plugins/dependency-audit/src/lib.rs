//! Dependency Audit for OpenIDEA: checks dependencies against OSV.dev.

#[allow(dead_code)]
mod manifest;
#[allow(dead_code)]
mod osv;

use openidea_plugin::providers::Providers;
use openidea_plugin::views::{View, Viewport, Views};
use openidea_plugin::{Error, Invocation, Plugin, register_plugin};

#[derive(Default)]
pub struct DependencyAudit;

impl Plugin for DependencyAudit {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
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
