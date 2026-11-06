//! What the Gradle and Maven build files around a Kotlin entity say about the `kotlin-jpa` compiler plugin.

use crate::model::Edit;

pub const BUILD_FILES: &[&str] = &["build.gradle.kts", "build.gradle", "pom.xml"];

const APPLIED: &[&str] = &["plugin.jpa", "kotlin-jpa", "kotlin.jpa", "kotlinJpa", "<plugin>jpa</plugin>"];

const KOTLIN: &[&str] = &["kotlin(\"jvm\")", "org.jetbrains.kotlin.jvm", "kotlin-gradle-plugin", "kotlin-maven-plugin"];

const NOT_LITERAL: &[&str] = &[
    "alias(", "buildSrc", "includeBuild", "subprojects", "allprojects", "apply(from", "apply from", "noArg", "no-arg", "plugin.noarg",
    "<parent>",
];

#[derive(Debug, PartialEq, Eq)]
pub enum Plugin {
    Applied,
    Missing,
    Unknown,
}

pub struct BuildFix {
    pub path: String,
    pub text: String,
    pub edit: Edit,
}

pub struct Build {
    pub plugin: Plugin,
    /// An `allopen` configuration names `Entity`, so `open` on an entity class is redundant.
    pub all_open_entities: bool,
    pub fix: Option<BuildFix>,
}

pub fn is_build_file(path: &str) -> bool {
    BUILD_FILES.iter().any(|name| path.rsplit('/').next() == Some(name))
}

fn without_comments(text: &str) -> String {
    text.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")
}

fn quoted_after(line: &str, keyword: &str) -> Option<String> {
    let rest = &line[line.find(keyword)? + keyword.len()..];
    let quote = rest.find(['"', '\''])?;
    let inner = &rest[quote + 1..];
    Some(inner[..inner.find(['"', '\''])?].to_owned())
}

fn gradle_fix(path: &str, text: &str) -> Option<BuildFix> {
    let mut offset = 0;

    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let at = offset;
        offset += line.len();

        let jvm = KOTLIN[..2].iter().any(|k| trimmed.contains(k)) && !trimmed.starts_with("//");
        let Some(version) = jvm.then(|| quoted_after(trimmed, "version")).flatten() else { continue };

        let indent = &line[..line.len() - trimmed.len()];
        let added = if path.ends_with(".kts") {
            format!("{indent}kotlin(\"plugin.jpa\") version \"{version}\"")
        } else {
            format!("{indent}id 'org.jetbrains.kotlin.plugin.jpa' version '{version}'")
        };
        let line_end = at + line.trim_end_matches(['\n', '\r']).len();

        return Some(BuildFix { path: path.into(), text: text.into(), edit: Edit::insert(line_end, &format!("\n{added}")) });
    }

    None
}

/// `files`: build files from the nearest directory to the project root, as (path, text).
pub fn analyse(files: &[(String, String)]) -> Build {
    let texts: Vec<String> = files.iter().map(|(_, t)| without_comments(t)).collect();
    let any = |markers: &[&str]| texts.iter().any(|t| markers.iter().any(|m| t.contains(m)));
    let all_open_entities = texts.iter().any(|t| (t.contains("allOpen") || t.contains("<plugin>all-open</plugin>")) && t.contains("persistence.Entity"));

    let plugin = if any(APPLIED) {
        Plugin::Applied
    } else if any(NOT_LITERAL) || !any(KOTLIN) {
        Plugin::Unknown
    } else {
        Plugin::Missing
    };

    let fix = (plugin == Plugin::Missing).then(|| files.iter().find_map(|(p, t)| gradle_fix(p, t))).flatten();

    Build { plugin, all_open_entities, fix }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(text: &str) -> Build {
        analyse(&[("/p/build.gradle.kts".into(), text.into())])
    }

    #[test]
    fn reads_the_plugin_by_literal_name() {
        assert_eq!(build("plugins {\n    kotlin(\"plugin.jpa\") version \"2.0.0\"\n}").plugin, Plugin::Applied);
        assert_eq!(build("apply plugin: 'kotlin-jpa'").plugin, Plugin::Applied);
        assert_eq!(analyse(&[("pom.xml".into(), "<plugin>jpa</plugin><artifactId>kotlin-maven-plugin</artifactId>".into())]).plugin, Plugin::Applied);
        assert_eq!(build("plugins {\n    kotlin(\"jvm\") version \"2.0.0\"\n}").plugin, Plugin::Missing);
        assert_eq!(build("// kotlin-jpa\nplugins {\n    kotlin(\"jvm\") version \"2.0.0\"\n}").plugin, Plugin::Missing);
    }

    #[test]
    fn unfamiliar_builds_stay_unknown() {
        assert_eq!(build("plugins {\n    alias(libs.plugins.kotlin.jvm)\n}").plugin, Plugin::Unknown);
        assert_eq!(build("plugins {\n    kotlin(\"jvm\") version \"2.0.0\"\n    id(\"company.conventions\")\n}\nsubprojects {}").plugin, Plugin::Unknown);
        assert_eq!(build("noArg { annotation(\"jakarta.persistence.Entity\") }\nkotlin(\"jvm\")").plugin, Plugin::Unknown);
        assert_eq!(build("plugins { java }").plugin, Plugin::Unknown);
    }

    #[test]
    fn the_fix_copies_the_kotlin_version_and_the_dsl() {
        let text = "plugins {\n    kotlin(\"jvm\") version \"2.0.21\"\n    id(\"x\")\n}\n";
        let fix = build(text).fix.unwrap();
        assert_eq!(fix.edit.start, "plugins {\n    kotlin(\"jvm\") version \"2.0.21\"".len());
        assert_eq!(fix.edit.text, "\n    kotlin(\"plugin.jpa\") version \"2.0.21\"");

        let groovy = analyse(&[("/p/build.gradle".into(), "plugins {\n  id 'org.jetbrains.kotlin.jvm' version '1.9.0'\n}\n".into())]).fix.unwrap();
        assert_eq!(groovy.edit.text, "\n  id 'org.jetbrains.kotlin.plugin.jpa' version '1.9.0'");
        assert!(build("plugins {\n    kotlin(\"jvm\")\n}").fix.is_none());
    }

    #[test]
    fn allopen_must_name_the_entity_annotation() {
        assert!(build("allOpen {\n  annotation(\"jakarta.persistence.Entity\")\n}").all_open_entities);
        assert!(!build("allOpen {\n  annotation(\"x.Y\")\n}").all_open_entities);
    }
}
