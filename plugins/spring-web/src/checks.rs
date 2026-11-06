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
}

impl Groups {
    pub fn from_settings() -> Self {
        let on = |key: &str| host::setting_bool(&format!("springWeb.{key}")).unwrap_or(true);
        Self { api_versioning: on("checkApiVersioning"), init_binder: on("checkInitBinder") }
    }
}

/// What the project's configuration files and sources say about API versioning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Versioning {
    pub is_config_file: bool,
    pub configured: bool,
}

/// Reads one source or `application*` file: `None` for a source that does not mention API versioning.
pub fn versioning(path: &str, text: &str) -> Option<Versioning> {
    let is_config_file = !path.ends_with(".java") && !path.ends_with(".kt");
    let lower = text.to_ascii_lowercase();
    let configured = lower.contains("apiversion") || lower.contains("api-version") || lower.contains("api_version");
    (is_config_file || configured).then_some(Versioning { is_config_file, configured })
}

/// Whether the project configures versioning; `None` unless every file was read and an `application*` file exists.
pub fn project_versioning(complete: bool, files: &[Versioning]) -> Option<bool> {
    if !complete || !files.iter().any(|f| f.is_config_file) {
        return None;
    }
    Some(files.iter().any(|f| f.configured))
}

pub fn check_java(file: &JavaFile, versioning_configured: Option<bool>, groups: Groups) -> Vec<Finding> {
    let mut out = Vec::new();
    for ty in file.types.iter().filter(|t| t.kind == "class") {
        if groups.api_versioning && versioning_configured == Some(false) {
            api_version_mismatch(file, ty, &mut out);
        }
        if groups.init_binder {
            init_binder_signature(file, ty, &mut out);
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

    fn run(text: &str, configured: Option<bool>) -> Vec<(&'static str, String)> {
        let found = check_java(&parse(text), configured, Groups { api_versioning: true, init_binder: true });
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
        let cfg = Versioning { is_config_file: true, configured: false };
        assert_eq!(project_versioning(true, &[cfg]), Some(false));
        assert_eq!(project_versioning(false, &[cfg]), None);
        assert_eq!(project_versioning(true, &[Versioning { is_config_file: false, configured: true }]), None);
        assert_eq!(project_versioning(true, &[cfg, Versioning { is_config_file: false, configured: true }]), Some(true));
    }

    const BINDERS: &str = "import org.springframework.web.bind.annotation.InitBinder;\n@Controller\nclass A {\n    @InitBinder\n    String bad(WebDataBinder b) { return \"\"; }\n    @InitBinder(\"x\")\n    void none(Locale l) {}\n    @InitBinder\n    void ok(WebDataBinder b, WebRequest r) {}\n}\n";

    #[test]
    fn init_binder_signature_warns() {
        assert_eq!(
            run(BINDERS, None),
            [("init-binder-signature", "bad".to_owned()), ("init-binder-signature", "none".to_owned())]
        );
        let found = check_java(&parse(BINDERS), None, Groups { api_versioning: true, init_binder: true });
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
        let off = Groups { api_versioning: false, init_binder: false };
        assert!(check_java(&parse(BINDERS), Some(false), off).is_empty());
    }
}
