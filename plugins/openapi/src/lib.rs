//! OpenAPI: editing support for OpenAPI 3 and Swagger 2 files.

#[allow(dead_code)]
mod spec;

use openidea_plugin::{Error, Invocation, Plugin, register_plugin};

#[derive(Default)]
pub struct OpenApi;

impl Plugin for OpenApi {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

register_plugin!(OpenApi);
