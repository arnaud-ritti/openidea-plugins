//! Flyway Migrations for OpenIDEA: Flyway's SQL migrations, read from file
//! names and project configuration (there is no database to ask).
//!
//! - Migration file names are checked against the configured naming scheme.

#[allow(dead_code)] // the checks that call it arrive with the next modules
mod naming;
#[allow(dead_code)] // consumed by the next tasks
mod config;
#[allow(dead_code)] // published by the next task
mod checks;

use openidea_plugin::{Error, Invocation, Plugin, register_plugin};

#[derive(Default)]
pub struct Flyway;

impl Plugin for Flyway {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

register_plugin!(Flyway);
