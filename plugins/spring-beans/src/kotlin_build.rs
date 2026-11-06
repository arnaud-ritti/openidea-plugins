//! A Kotlin class Spring must proxy (or subclass) has to be `open`; the
//! `kotlin-spring` compiler plugin opens classes carrying Spring annotations.
//! Quiet when the module has no build file or already names the plugin.

use crate::checks::{BUILD_FILES, build_module, spring};
use openidea_plugin::TextEdit;
use spring_text::finding::Finding;
use spring_text::java::{JavaFile, TypeDecl};
use spring_text::range;

const OPEN_MODIFIERS: [&str; 8] = ["open", "abstract", "sealed", "data", "enum", "inner", "annotation", "value"];
const OPENING_ANNOTATIONS: [&str; 6] = ["Transactional", "Async", "Cacheable", "CacheEvict", "CachePut", "Retryable"];
const PLUGIN_MARKERS: [&str; 3] = ["kotlin-spring", "plugin.spring", "allopen"];
const KTS_JVM_MARKERS: [&str; 2] = ["kotlin(\"jvm\")", "\"org.jetbrains.kotlin.jvm\""];
const GROOVY_JVM_MARKER: &str = "org.jetbrains.kotlin.jvm";

pub fn missing_spring_plugin(path: &str, file: &JavaFile) -> Vec<Finding> {
    let candidates: Vec<&TypeDecl> = file.types.iter().filter(|t| t.kind == "class" && needs_plugin(file, t)).collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    let builds = module_builds(path);
    if builds.is_empty() || builds.iter().any(|(_, text)| names_plugin(text)) {
        return Vec::new();
    }
    candidates
        .into_iter()
        .map(|ty| {
            Finding::new(
                ty.at,
                ty.at + ty.name.len(),
                "kotlin-spring-plugin",
                format!(
                    "Kotlin classes are final, but Spring has to proxy `{}` and can't subclass a final class. Make it open or apply the kotlin-spring compiler plugin, which opens classes carrying Spring annotations.",
                    ty.name
                ),
            )
        })
        .collect()
}

fn needs_plugin(file: &JavaFile, ty: &TypeDecl) -> bool {
    if ty.modifiers.iter().any(|m| OPEN_MODIFIERS.contains(&m.as_str())) {
        return false;
    }
    if let Some(configuration) = spring(file, &ty.annotations, "Configuration") {
        let proxied = configuration.arg("proxyBeanMethods").is_none_or(|a| a.text.trim() == "true");
        if proxied {
            return true;
        }
    }
    OPENING_ANNOTATIONS.iter().any(|name| {
        spring(file, &ty.annotations, name).is_some() || ty.members.iter().any(|m| spring(file, &m.annotations, name).is_some())
    })
}

/// The text of every build file in the module holding `path`, with its full path.
fn module_builds(path: &str) -> Vec<(String, String)> {
    let Some(module) = build_module(path) else { return Vec::new() };
    let Some(project) = spring_text::index::project_of(path) else { return Vec::new() };
    BUILD_FILES
        .iter()
        .filter_map(|f| {
            let full = format!("{module}/{f}");
            let text = project.read_file(&full).ok()?;
            Some((full, text))
        })
        .collect()
}

fn names_plugin(build: &str) -> bool {
    if PLUGIN_MARKERS.iter().any(|m| build.contains(m)) {
        return true;
    }
    let Some(from) = build.find("compilerPlugins") else { return false };
    let rest = &build[from..];
    let block = match rest.find("</compilerPlugins>") {
        Some(end) => &rest[..end],
        None => bracketed(rest).unwrap_or(rest.lines().next().unwrap_or("")),
    };
    ["\"spring\"", "'spring'", ">spring<"].iter().any(|m| block.contains(m))
}

/// The text from the first opening bracket to its match.
fn bracketed(text: &str) -> Option<&str> {
    let open = text.find(['{', '(', '['])?;
    let mut depth = 0;
    for (i, c) in text[open..].char_indices() {
        match c {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[open..open + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The Gradle build file of the module holding `path` and the edit applying the plugin there.
pub fn fix_for(path: &str) -> Option<(String, TextEdit)> {
    module_builds(path).into_iter().find_map(|(build_path, text)| {
        let edit = gradle_fix(&build_path, &text)?;
        Some((build_path, edit))
    })
}

pub fn gradle_fix(build_path: &str, build_text: &str) -> Option<TextEdit> {
    let kotlin_dsl = build_path.ends_with(".gradle.kts");
    if !kotlin_dsl && !build_path.ends_with(".gradle") {
        return None;
    }
    let (open, close) = plugins_block(build_text)?;
    let closing_line = build_text[..close].rfind('\n')? + 1;
    if closing_line <= open || !build_text[closing_line..close].trim().is_empty() {
        return None;
    }
    let jvm_line = build_text[open..close].lines().find(|line| {
        match kotlin_dsl {
            true => KTS_JVM_MARKERS.iter().any(|m| line.contains(m)),
            false => line.contains(GROOVY_JVM_MARKER),
        }
    })?;
    let version = quoted_version(jvm_line)?;
    let indent: String = jvm_line.chars().take_while(|c| c.is_whitespace()).collect();
    let newline = if build_text.contains("\r\n") { "\r\n" } else { "\n" };
    let plugin = match kotlin_dsl {
        true => format!("kotlin(\"plugin.spring\") version \"{version}\""),
        false => format!("id 'org.jetbrains.kotlin.plugin.spring' version '{version}'"),
    };
    Some(TextEdit { range: range(build_text, closing_line, closing_line), text: format!("{indent}{plugin}{newline}") })
}

/// Byte offsets of the braces of the first top-level `plugins { ... }` block.
fn plugins_block(text: &str) -> Option<(usize, usize)> {
    let mut line_start = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let after = trimmed.strip_prefix("plugins").map(str::trim_start);
        if let Some(rest) = after.filter(|r| r.starts_with('{')) {
            let open = line_start + (line.len() - rest.len());
            let close = open + bracketed(&text[open..])?.len() - 1;
            return Some((open, close));
        }
        line_start += line.len();
    }
    None
}

fn quoted_version(line: &str) -> Option<&str> {
    let rest = &line[line.find("version")? + "version".len()..];
    let quote_at = rest.find(['"', '\''])?;
    let quote = rest[quote_at..].chars().next()?;
    let value = &rest[quote_at + 1..];
    let version = &value[..value.find(quote)?];
    (!version.is_empty() && !version.contains('$') && rest[..quote_at].trim().is_empty()).then_some(version)
}
