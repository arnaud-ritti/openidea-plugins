//! Text checks over one Java controller. A check stays quiet when an
//! annotation comes from a library other than Spring or the project's
//! configuration is not fully visible.

use crate::MAPPINGS;
use openidea_plugin::{Severity, host};
use spring_text::finding::Finding;
use spring_text::java::{JavaFile, Member, TypeDecl};

const DATA_BINDERS: [&str; 4] = ["WebDataBinder", "DataBinder", "ServletRequestDataBinder", "WebExchangeDataBinder"];

/// Which groups of checks run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Groups {
    pub api_versioning: bool,
    pub init_binder: bool,
    pub version_format: bool,
}

impl Groups {
    pub fn from_settings() -> Self {
        let on = |key: &str| host::setting_bool(&format!("springWeb.{key}")).unwrap_or(true);
        Self { api_versioning: on("checkApiVersioning"), init_binder: on("checkInitBinder"), version_format: on("checkVersionFormat") }
    }
}

/// What the project's configuration files and sources say about API versioning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Versioning {
    pub is_config_file: bool,
    pub configured: bool,
    pub custom_parser: bool,
}

/// Reads one source or `application*` file: `None` for a source that does not mention API versioning.
pub fn versioning(path: &str, text: &str) -> Option<Versioning> {
    let is_config_file = !path.ends_with(".java") && !path.ends_with(".kt");
    let lower = text.to_ascii_lowercase();
    let configured = lower.contains("apiversion") || lower.contains("api-version") || lower.contains("api_version");
    let custom_parser = text.contains("ApiVersionParser");
    (is_config_file || configured).then_some(Versioning { is_config_file, configured, custom_parser })
}

/// Whether the project configures versioning; `None` unless every file was read and an `application*` file exists.
pub fn project_versioning(complete: bool, files: &[Versioning]) -> Option<bool> {
    if !complete || !files.iter().any(|f| f.is_config_file) {
        return None;
    }
    Some(files.iter().any(|f| f.configured))
}

/// Whether the project uses Spring's own version parser; `None` unless every file was read.
pub fn project_standard_parser(complete: bool, files: &[Versioning]) -> Option<bool> {
    complete.then(|| files.iter().all(|f| !f.custom_parser))
}

pub fn check_java(file: &JavaFile, versioning_configured: Option<bool>, standard_parser: Option<bool>, groups: Groups) -> Vec<Finding> {
    let mut out = Vec::new();
    for ty in file.types.iter().filter(|t| t.kind == "class") {
        if groups.api_versioning && versioning_configured == Some(false) {
            api_version_mismatch(file, ty, &mut out);
        }
        if groups.init_binder {
            init_binder_signature(file, ty, &mut out);
        }
        if groups.version_format && standard_parser == Some(true) {
            invalid_api_version(file, ty, &mut out);
        }
    }
    out
}

fn from_spring(file: &JavaFile, name: &str) -> bool {
    let suffix = format!(".{name}");
    file.imports
        .iter()
        .filter(|i| !i.is_static && i.path.ends_with(&suffix))
        .all(|i| i.path.starts_with("org.springframework."))
}

/// Spring Framework 7 routes a request by `version` only when an API version strategy is configured; without one the attribute is ignored.
fn api_version_mismatch(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    let annotations = ty.annotations.iter().chain(ty.members.iter().flat_map(|m| &m.annotations));
    for a in annotations.filter(|a| MAPPINGS.contains(&a.name.as_str())) {
        let Some(version) = a.arg("version").filter(|_| from_spring(file, &a.name)) else { continue };
        out.push(Finding::new(
            version.at,
            version.end,
            "api-version-mismatch",
            "This mapping uses `version`, but no API versioning is configured (spring.mvc.apiversion.* or ApiVersionConfigurer), so the attribute has no effect.",
        ));
    }
}

fn is_semantic_version(text: &str) -> bool {
    let core = text.strip_suffix('+').unwrap_or(text);
    let core = core.strip_prefix('v').unwrap_or(core);
    let parts: Vec<_> = core.split('.').collect();

    parts.len() <= 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The default parser reads `major[.minor[.patch]]`; a literal that is not one never matches a request.
fn invalid_api_version(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    let annotations = ty.annotations.iter().chain(ty.members.iter().flat_map(|m| &m.annotations));
    for a in annotations.filter(|a| MAPPINGS.contains(&a.name.as_str())) {
        let Some(version) = a.arg("version").filter(|_| from_spring(file, &a.name)) else { continue };
        let [literal] = version.strings.as_slice() else { continue };
        let alone = version.text.starts_with('"') && version.text.ends_with('"');
        if !alone || is_semantic_version(&literal.value) {
            continue;
        }

        out.push(Finding::new(
            version.at,
            version.end,
            "invalid-api-version",
            "This is not a version the default parser reads; use `major`, `major.minor` or `major.minor.patch`, optionally ending in `+` for a baseline.",
        ));
    }
}

fn init_binder_signature(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    for m in ty.methods().filter(|m| m.annotation("InitBinder").is_some() && from_spring(file, "InitBinder")) {
        let end = m.at + m.name.len();
        if m.ty.as_ref().is_some_and(|t| t.name != "void") {
            out.push(
                Finding::new(m.at, end, "init-binder-signature", "@InitBinder methods must return void; Spring throws IllegalStateException otherwise.")
                    .with_severity(Severity::Error),
            );
            continue;
        }
        if !has_binder(m) {
            out.push(Finding::new(
                m.at,
                end,
                "init-binder-signature",
                "An @InitBinder method takes a WebDataBinder parameter to register editors, formatters or validators.",
            ));
        }
    }
}

fn has_binder(method: &Member) -> bool {
    method.params.iter().any(|p| DATA_BINDERS.contains(&p.ty.name.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use spring_text::java::parse;

    const ALL: Groups = Groups { api_versioning: true, init_binder: true, version_format: true };

    fn run(text: &str, configured: Option<bool>) -> Vec<(&'static str, String)> {
        run_with(text, configured, Some(true), ALL)
    }

    fn run_with(text: &str, configured: Option<bool>, parser: Option<bool>, groups: Groups) -> Vec<(&'static str, String)> {
        let found = check_java(&parse(text), configured, parser, groups);
        found.iter().map(|f| (f.code, text[f.start..f.end].to_owned())).collect()
    }

    const VERSIONED: &str = "import org.springframework.web.bind.annotation.*;\n@RestController\nclass A {\n    @GetMapping(path = \"/a\", version = \"1.1\")\n    String a() { return \"\"; }\n}\n";

    #[test]
    fn api_version_mismatch_warns() {
        assert_eq!(run(VERSIONED, Some(false)), [("api-version-mismatch", "\"1.1\"".to_owned())]);
    }

    #[test]
    fn api_version_mismatch_is_quiet_for_library_or_dynamic_name() {
        assert!(run(VERSIONED, Some(true)).is_empty());
        assert!(run(VERSIONED, None).is_empty());
        let foreign = VERSIONED.replace("org.springframework.web.bind.annotation.*", "org.acme.GetMapping");
        assert!(run(&foreign, Some(false)).is_empty());
        assert!(run(&VERSIONED.replace("version = \"1.1\"", "produces = \"x\""), Some(false)).is_empty());
    }

    #[test]
    fn versioning_is_read_from_config_and_sources() {
        let none = |p, t| versioning(p, t).map(|v| v.configured);
        assert_eq!(none("/p/application.yml", "server:\n  port: 1\n"), Some(false));
        assert_eq!(none("/p/application.yml", "spring:\n  mvc:\n    apiversion:\n      use:\n        header: X\n"), Some(true));
        assert_eq!(none("/p/application.properties", "spring.mvc.apiversion.use.header=X"), Some(true));
        assert_eq!(none("/p/A.java", "class A {}"), None);
        assert_eq!(none("/p/C.java", "class C implements WebMvcConfigurer { void configureApiVersioning(ApiVersionConfigurer c) {} }"), Some(true));
        let cfg = Versioning { is_config_file: true, configured: false, custom_parser: false };
        assert_eq!(project_versioning(true, &[cfg]), Some(false));
        assert_eq!(project_versioning(false, &[cfg]), None);
        assert_eq!(project_versioning(true, &[Versioning { is_config_file: false, configured: true, custom_parser: false }]), None);
        assert_eq!(project_versioning(true, &[cfg, Versioning { is_config_file: false, configured: true, custom_parser: false }]), Some(true));
    }

    const BINDERS: &str = "import org.springframework.web.bind.annotation.InitBinder;\n@Controller\nclass A {\n    @InitBinder\n    String bad(WebDataBinder b) { return \"\"; }\n    @InitBinder(\"x\")\n    void none(Locale l) {}\n    @InitBinder\n    void ok(WebDataBinder b, WebRequest r) {}\n}\n";

    #[test]
    fn init_binder_signature_warns() {
        assert_eq!(
            run(BINDERS, None),
            [("init-binder-signature", "bad".to_owned()), ("init-binder-signature", "none".to_owned())]
        );
        let found = check_java(&parse(BINDERS), None, None, ALL);
        assert_eq!(found[0].severity, Severity::Error);
        assert_eq!(found[1].severity, Severity::Warning);
    }

    #[test]
    fn init_binder_signature_is_quiet_for_library_or_dynamic_name() {
        let foreign = BINDERS.replace("org.springframework.web.bind.annotation.InitBinder", "org.acme.InitBinder");
        assert!(run(&foreign, None).is_empty());
        let fixed = "class A {\n    @InitBinder\n    void ok(DataBinder b) {}\n}\n";
        assert!(run(fixed, None).is_empty());
    }

    #[test]
    fn groups_can_be_switched_off() {
        let off = Groups { api_versioning: false, init_binder: false, version_format: false };
        assert!(check_java(&parse(BINDERS), Some(false), Some(true), off).is_empty());
    }

    fn versions(version: &str) -> Vec<(&'static str, String)> {
        run(&VERSIONED.replace("\"1.1\"", version), Some(true))
    }

    #[test]
    fn malformed_version_warns() {
        for bad in ["\"1.x\"", "\"latest\"", "\"1..2\""] {
            assert_eq!(versions(bad), [("invalid-api-version", bad.to_owned())]);
        }
    }

    #[test]
    fn semantic_versions_are_quiet() {
        for good in ["\"1\"", "\"1.1\"", "\"1.2.3\"", "\"1.2+\"", "\"v2\""] {
            assert!(versions(good).is_empty(), "{good}");
        }
    }

    #[test]
    fn version_format_is_quiet_for_library_or_dynamic_name() {
        assert!(versions("VERSION").is_empty());
        assert!(versions("{\"1.x\", \"2\"}").is_empty());
        assert!(versions("\"1.\" + \"x\"").is_empty());
        assert!(versions("V + \"x\" + V").is_empty());
        assert!(versions("A+\"x\"+B").is_empty());
        assert!(versions("\"x\" + V").is_empty());
        let bad = VERSIONED.replace("\"1.1\"", "\"latest\"");
        assert!(run_with(&bad, Some(true), Some(false), ALL).is_empty());
        assert!(run_with(&bad, Some(true), None, ALL).is_empty());
        let foreign = bad.replace("org.springframework.web.bind.annotation.*", "org.acme.GetMapping");
        assert!(run(&foreign, Some(true)).is_empty());
    }

    #[test]
    fn standard_parser_needs_every_file_without_a_custom_one() {
        let plain = Versioning::default();
        let custom = Versioning { custom_parser: true, ..plain };
        assert_eq!(project_standard_parser(true, &[plain]), Some(true));
        assert_eq!(project_standard_parser(true, &[plain, custom]), Some(false));
        assert_eq!(project_standard_parser(false, &[plain]), None);
        assert!(versioning("/p/P.java", "class P implements ApiVersionParser<String> {}").unwrap().custom_parser);
    }

    #[test]
    fn version_format_group_can_be_switched_off() {
        let off = Groups { version_format: false, ..ALL };
        let bad = VERSIONED.replace("\"1.1\"", "\"latest\"");
        assert!(run_with(&bad, Some(true), Some(true), off).is_empty());
    }

    #[test]
    fn fixed_file_publishes_empty_list() {
        let fixed = VERSIONED.replace("\"1.1\"", "\"1.2\"");
        assert!(run(&fixed, Some(true)).is_empty());
    }

    #[test]
    fn ranges_use_utf16_columns_with_multibyte_text() {
        let text = VERSIONED.replace("path = \"/a\"", "path = \"/é😀\"").replace("\"1.1\"", "\"é1\"");
        let found = check_java(&parse(&text), Some(true), Some(true), ALL);
        let range = found[0].diagnostic(&text).range;
        assert_eq!(range.start.line, 3);
        assert_eq!(range.start.character, "    @GetMapping(path = \"/é😀\", version = ".encode_utf16().count() as u32);
        assert_eq!(range.end.character - range.start.character, 4);
    }

    #[test]
    fn crlf_files_report_correct_lines() {
        let text = VERSIONED.replace("1.1", "latest").replace('\n', "\r\n");
        let found = check_java(&parse(&text), Some(true), Some(true), ALL);
        let range = found[0].diagnostic(&text).range;
        assert_eq!(range.start.line, 3);
        assert_eq!(range.end.line, 3);
        assert_eq!(range.end.character - range.start.character, "\"latest\"".len() as u32);
    }
}
