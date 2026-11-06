//! Catalogue of start.spring.io (dependency and Spring Boot version lists) and
//! the dependency text Add Starters inserts into an open build file.

use openidea_plugin::{Error, HttpRequest, PickItem, host};
use serde_json::Value;

const ACCEPT: &str = "application/vnd.initializr.v2.2+json";

pub struct Coordinates {
    pub group: String,
    pub artifact: String,
    pub scope: String,
    pub version: Option<String>,
}

fn get(path: &str) -> Result<Value, Error> {
    let url = format!("{}{path}", crate::START);
    let response = host::fetch(&HttpRequest::get(&url).header("Accept", ACCEPT))?;
    if response.status != 200 {
        return Err(Error::Failed(format!("{url} answered {}", response.status)));
    }

    serde_json::from_slice(&response.body).map_err(|e| Error::Failed(format!("unreadable answer from {url}: {e}")))
}

fn item(value: &Value, group: Option<&str>) -> Option<PickItem> {
    let id = value["id"].as_str()?;
    let name = value["name"].as_str().unwrap_or(id);
    let description = [group, value["description"].as_str()].into_iter().flatten().collect::<Vec<_>>().join(": ");
    let item = PickItem::new(id, name);
    Some(if description.is_empty() { item } else { item.description(&description) })
}

pub fn dependency_items() -> Result<Vec<PickItem>, Error> {
    let metadata = get("/metadata/client")?;
    let groups = metadata["dependencies"]["values"].as_array().map_or(&[][..], Vec::as_slice);
    Ok(groups
        .iter()
        .flat_map(|g| {
            let name = g["name"].as_str();
            g["values"].as_array().into_iter().flatten().filter_map(move |d| item(d, name))
        })
        .collect())
}

pub fn boot_version_items() -> Result<Vec<PickItem>, Error> {
    let metadata = get("/metadata/client")?;
    Ok(metadata["bootVersion"]["values"].as_array().into_iter().flatten().filter_map(|v| item(v, None)).collect())
}

/// Coordinates of each id the server can express as a plain dependency, and
/// the ids it cannot (a BOM or repository is needed).
pub fn coordinates(ids: &[String], boot_version: &str) -> Result<(Vec<Coordinates>, Vec<String>), Error> {
    let query = if boot_version.is_empty() { String::new() } else { format!("?bootVersion={boot_version}") };
    let all = get(&format!("/dependencies{query}"))?;
    let (mut found, mut skipped) = (Vec::new(), Vec::new());
    for id in ids {
        let d = &all["dependencies"][id];
        let plain = d["bom"].is_null() && d["repository"].is_null();
        match (d["groupId"].as_str(), d["artifactId"].as_str()) {
            (Some(group), Some(artifact)) if plain => found.push(Coordinates {
                group: group.into(),
                artifact: artifact.into(),
                scope: d["scope"].as_str().unwrap_or("compile").into(),
                version: d["version"].as_str().map(Into::into),
            }),
            _ => skipped.push(id.clone()),
        }
    }
    Ok((found, skipped))
}

fn is_gradle(path: &str) -> bool {
    path.ends_with(".gradle") || path.ends_with(".gradle.kts")
}

pub fn supports(path: &str) -> bool {
    is_gradle(path) || path.ends_with("pom.xml")
}

/// Byte offset to insert at and the text, or why the file cannot take dependencies.
pub fn insertion(path: &str, text: &str, deps: &[Coordinates]) -> Result<(usize, String), String> {
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let (at, indent, entries): (usize, String, Vec<String>) = if is_gradle(path) {
        let (at, indent) = gradle_block_end(text).ok_or("Found no top-level `dependencies { }` block.")?;
        let quote = if path.ends_with(".kts") { '"' } else { '\'' };
        let entries = deps.iter().filter(|d| !text.contains(&format!("{}:{}", d.group, d.artifact))).map(|d| gradle_line(d, quote));
        (at, indent, entries.collect())
    } else if path.ends_with("pom.xml") {
        let (at, indent, step) = maven_block_end(text).ok_or("Found no project `<dependencies>` element with its closing tag on its own line.")?;
        let entries = deps.iter().filter(|d| !text.contains(&format!("<artifactId>{}</artifactId>", d.artifact))).map(|d| maven_xml(d, &indent, &step, newline));
        (at, indent.clone(), entries.collect())
    } else {
        return Err("Open a pom.xml, build.gradle or build.gradle.kts.".into());
    };
    if entries.is_empty() {
        return Err("Every selected starter is already in the file.".into());
    }

    Ok((at, entries.iter().map(|e| format!("{indent}{e}{newline}")).collect()))
}

fn blank(s: &str) -> bool {
    s.chars().all(char::is_whitespace)
}

fn maven_block_end(text: &str) -> Option<(usize, String, String)> {
    let limit = ["<build>", "<profiles>"].iter().filter_map(|t| text.find(t)).min().unwrap_or(text.len());
    let managed = text.find("<dependencyManagement>").zip(text.find("</dependencyManagement>"));
    let mut from = 0;
    while let Some(found) = text[from..limit].find("</dependencies>") {
        let pos = from + found;
        from = pos + 1;
        if managed.is_some_and(|(a, b)| a < pos && pos < b) {
            continue;
        }

        let line_start = text[..pos].rfind('\n').map_or(0, |n| n + 1);
        let closing = &text[line_start..pos];
        if !blank(closing) {
            return None;
        }

        let opening = text[..pos].rfind("<dependencies>").unwrap_or(0);
        let existing = text[opening..pos].find("<dependency>").map(|n| {
            let at = opening + n;
            let start = text[..at].rfind('\n').map_or(0, |l| l + 1);
            text[start..at].to_owned()
        });
        let indent = existing.filter(|i| blank(i)).unwrap_or_else(|| format!("{closing}    "));
        let step = indent.strip_prefix(closing).filter(|d| !d.is_empty()).unwrap_or("    ").to_owned();
        return Some((line_start, indent, step));
    }

    None
}

fn gradle_block_end(text: &str) -> Option<(usize, String)> {
    let mut offset = 0;
    let mut open = false;
    let mut indent = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end();
        if open && trimmed == "}" {
            return Some((offset, indent.unwrap_or_else(|| "    ".into())));
        }

        if open && indent.is_none() && !blank(line) {
            indent = Some(line.chars().take_while(|c| c.is_whitespace()).collect());
        }

        if !open && (trimmed.starts_with("dependencies {") || trimmed.starts_with("dependencies{")) {
            open = true;
        }

        offset += line.len();
    }

    None
}

fn gradle_line(d: &Coordinates, quote: char) -> String {
    let configuration = match d.scope.as_str() {
        "runtime" => "runtimeOnly",
        "provided" | "compileOnly" => "compileOnly",
        "annotationProcessor" => "annotationProcessor",
        "test" => "testImplementation",
        _ => "implementation",
    };
    let version = d.version.as_ref().map_or(String::new(), |v| format!(":{v}"));
    format!("{configuration} {quote}{}:{}{version}{quote}", d.group, d.artifact)
}

fn maven_xml(d: &Coordinates, indent: &str, step: &str, newline: &str) -> String {
    let inner = format!("{indent}{step}");
    let mut lines = vec![
        "<dependency>".to_owned(),
        format!("{inner}<groupId>{}</groupId>", d.group),
        format!("{inner}<artifactId>{}</artifactId>", d.artifact),
    ];
    if let Some(version) = &d.version {
        lines.push(format!("{inner}<version>{version}</version>"));
    }
    if matches!(d.scope.as_str(), "runtime" | "test" | "provided") {
        lines.push(format!("{inner}<scope>{}</scope>", d.scope));
    }
    lines.push(format!("{indent}</dependency>"));
    lines.join(newline)
}
