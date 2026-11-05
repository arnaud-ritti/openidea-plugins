//! Helpers for resolving bean names written in `ref`-style attributes.

/// Index globs of the sources that declare beans by name.
pub const GLOBS: [&str; 3] = ["**/*.java", "**/*.kt", "**/*.xml"];

/// Bean names that libraries and auto-configuration usually provide: a
/// reference to one is never reported as unknown.
pub const INFRASTRUCTURE: [&str; 11] = [
    "transactionManager",
    "jobRepository",
    "dataSource",
    "entityManagerFactory",
    "taskExecutor",
    "jobLauncher",
    "errorChannel",
    "nullChannel",
    "integrationConversionService",
    "messageBuilderFactory",
    "conversionService",
];

/// Whether `path` is a build file, build output or a dependency rather than
/// a source: `pom.xml`, `target/`, `build/` and `node_modules/`.
pub fn ignored(path: &str) -> bool {
    let path = path.replace('\\', "/");
    // ponytail: a `src/` tree is never output, so a project under `~/build/`
    // or a `build` package is still read; a root-aware check if that misfires.
    let output = !path.contains("/src/") && (path.contains("/target/") || path.contains("/build/"));
    path.ends_with("/pom.xml") || path == "pom.xml" || path.contains("/node_modules/") || output
}

/// Whether a name is a `${…}` placeholder or a `#{…}` expression (never resolved).
pub fn is_expression(name: &str) -> bool {
    name.contains("${") || name.contains("#{")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_build_files_and_output() {
        assert!(ignored("/p/pom.xml"));
        assert!(ignored("/p/target/classes/context.xml"));
        assert!(ignored("/p/build/resources/main/context.xml"));
        assert!(ignored("/p/node_modules/a/src/x.xml"));
        assert!(!ignored("/home/build/p/src/main/resources/context.xml"));
        assert!(!ignored("/p/src/main/java/com/acme/build/Job.java"));
        assert!(is_expression("${channel}") && is_expression("#{bean}") && !is_expression("orders"));
    }
}
