//! Quarkus Tools for OpenIDEA: completion and checks for Quarkus configuration
//! files, ported from Quarkus Tools for IntelliJ 2.6.1 (Red Hat, EPL-2.0).

use openidea_plugin::{Error, Invocation, Plugin, register_plugin};
use spring_text::metadata::{Catalog, parse_tsv};
use std::sync::OnceLock;

/// Quarkus 3.40.1 and MicroProfile properties.
pub fn bundled() -> &'static Catalog {
    static BUNDLED: OnceLock<Catalog> = OnceLock::new();
    BUNDLED.get_or_init(|| {
        let mut c = Catalog::default();
        c.add(&parse_tsv(include_str!("../data/quarkus-properties.tsv")));
        c
    })
}

#[derive(Default)]
pub struct QuarkusTools;

impl Plugin for QuarkusTools {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

register_plugin!(QuarkusTools);

#[cfg(test)]
mod tests {
    use super::*;
    use spring_text::metadata::Lookup;

    #[test]
    fn reads_the_bundled_metadata() {
        let lookup = Lookup(vec![bundled()]);
        let port = lookup.prop("quarkus.http.port").unwrap();
        assert_eq!((port.ty.as_str(), port.default.as_str(), port.description.as_str()), ("int", "8080", "The HTTP port"));
        assert!(lookup.prop("quarkus.hibernate-orm.database.generation").unwrap().deprecated.is_some());
        assert_eq!(lookup.values("quarkus.http.ssl.client-auth"), ["none", "request", "required"]);
        assert_eq!(lookup.values("quarkus.http.cors.enabled"), ["true", "false"]);
        assert!(lookup.values("quarkus.log.level").contains(&"DEBUG".to_owned()));
        assert!(lookup.prop("mp.jwt.verify.publickey").is_some());
        assert!(lookup.prop("quarkus.datasource.\"datasource-name\".db-kind").is_none());
    }
}
