//! What the project's build files declare: JDBC drivers and the Reactor debug agent.
//! Read as text, so only dependencies written in the build files are seen.

use spring_text::finding::Finding;
use openidea_plugin::Severity;
use std::collections::BTreeSet;

pub const BUILD_GLOBS: &[&str] = &["**/pom.xml", "**/build.gradle", "**/build.gradle.kts", "**/libs.versions.toml"];

/// JDBC URL scheme → text that names its driver in a build file.
const DRIVERS: &[(&str, &[&str])] = &[
    ("postgresql", &["postgresql"]),
    ("mysql", &["mysql-connector", "mysql:mysql"]),
    ("mariadb", &["mariadb-java-client"]),
    ("sqlserver", &["mssql-jdbc"]),
    ("oracle", &["ojdbc"]),
    ("h2", &["h2database", ":h2", ">h2<", "\"h2\""]),
    ("hsqldb", &["hsqldb"]),
    ("sqlite", &["sqlite-jdbc"]),
    ("derby", &["derby"]),
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub drivers: BTreeSet<&'static str>,
    pub reactor_agent: bool,
}

pub fn read(_path: &str, text: &str) -> Option<Facts> {
    let lower = text.to_ascii_lowercase();
    let drivers = DRIVERS.iter().filter(|(_, names)| names.iter().any(|n| lower.contains(n))).map(|(scheme, _)| *scheme).collect();
    Some(Facts { drivers, reactor_agent: lower.contains("reactor-tools") })
}

/// The build files of a project, merged; `None` until every one was read.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub drivers: BTreeSet<&'static str>,
    pub reactor_agent: bool,
}

impl Summary {
    pub fn merge<'a>(facts: impl Iterator<Item = &'a Facts>) -> Self {
        facts.fold(Self::default(), |mut all, f| {
            all.drivers.extend(f.drivers.iter().copied());
            all.reactor_agent |= f.reactor_agent;
            all
        })
    }
}

/// The driver a `jdbc:` URL needs when the build files declare no driver for it.
pub fn missing_driver(url: &str, build: &Summary) -> Option<String> {
    let scheme = url.strip_prefix("jdbc:")?.split(':').next()?;
    let (known, _) = DRIVERS.iter().find(|(s, _)| *s == scheme)?;
    (!build.drivers.contains(known)).then(|| {
        format!("No `{known}` JDBC driver is declared in the build files; it may come in transitively.")
    })
}

/// `Hooks.onOperatorDebug()` calls when the Reactor debug agent is a dependency.
pub fn operator_debug(text: &str, build: &Summary) -> Vec<Finding> {
    if !build.reactor_agent {
        return Vec::new();
    }
    text.match_indices("Hooks.onOperatorDebug()")
        .map(|(at, call)| {
            Finding::new(at, at + call.len(), "reactor-debug-agent", "`reactor-tools` is a dependency: `ReactorDebugAgent.init()` gives the same assembly traces without the runtime cost.")
                .with_severity(Severity::Hint)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(build: &str) -> Summary {
        Summary::merge(read("pom.xml", build).iter())
    }

    #[test]
    fn driver_missing_only_when_none_is_declared() {
        let pg = summary("<artifactId>postgresql</artifactId>");
        assert!(missing_driver("jdbc:postgresql://db/app", &pg).is_none());
        assert!(missing_driver("jdbc:mysql://db/app", &pg).is_some());
        assert!(missing_driver("jdbc:unknown:x", &pg).is_none());
        assert!(missing_driver("${URL}", &pg).is_none());
    }

    #[test]
    fn operator_debug_needs_the_agent() {
        let text = "void f() { Hooks.onOperatorDebug(); }";
        assert!(operator_debug(text, &summary("<artifactId>spring-boot</artifactId>")).is_empty());
        let found = operator_debug(text, &summary("<artifactId>reactor-tools</artifactId>"));
        assert_eq!((found.len(), found[0].severity), (1, Severity::Hint));
    }
}
