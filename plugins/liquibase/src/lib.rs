//! Liquibase Changelogs: an OpenIDEA plugin.

#[allow(dead_code)]
mod catalog;

use openidea_plugin::providers::Providers;
use openidea_plugin::{Error, Invocation, Plugin, register_plugin};

#[derive(Default)]
pub struct Liquibase;

impl Plugin for Liquibase {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

impl Providers for Liquibase {}

register_plugin!(Liquibase, providers);
