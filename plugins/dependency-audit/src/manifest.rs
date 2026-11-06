//! Dependencies declared by the project manifests and lockfiles.

use std::collections::HashMap;

use serde_json::Value;
use spring_text::xml::{Element, elements};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ecosystem {
    Maven,
    Npm,
    Packagist,
    CratesIo,
    PyPI,
}

impl Ecosystem {
    pub fn osv(self) -> &'static str {
        match self {
            Ecosystem::Maven => "Maven",
            Ecosystem::Npm => "npm",
            Ecosystem::Packagist => "Packagist",
            Ecosystem::CratesIo => "crates.io",
            Ecosystem::PyPI => "PyPI",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Pom,
    Gradle,
    PackageJson,
    PackageLock,
    ComposerJson,
    ComposerLock,
    VersionCatalog,
    YarnLock,
    PnpmLock,
    CargoLock,
    PoetryLock,
    Requirements,
}

pub const GLOBS: &[&str] = &[
    "**/pom.xml",
    "**/build.gradle",
    "**/build.gradle.kts",
    "**/package.json",
    "**/composer.json",
    "**/package-lock.json",
    "**/composer.lock",
    "**/libs.versions.toml",
    "**/yarn.lock",
    "**/pnpm-lock.yaml",
    "**/Cargo.lock",
    "**/poetry.lock",
    "**/requirements.txt",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dep {
    pub path: String,
    pub ecosystem: Ecosystem,
    pub name: String,
    pub version: String,
    /// Bytes the diagnostic underlines.
    pub at: (usize, usize),
    /// Bytes the quick fix replaces; `None` in lockfiles.
    pub version_at: Option<(usize, usize)>,
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn kind(path: &str) -> Option<Kind> {
    match file_name(path) {
        "pom.xml" => Some(Kind::Pom),
        "build.gradle" | "build.gradle.kts" => Some(Kind::Gradle),
        "package.json" => Some(Kind::PackageJson),
        "package-lock.json" => Some(Kind::PackageLock),
        "composer.json" => Some(Kind::ComposerJson),
        "composer.lock" => Some(Kind::ComposerLock),
        "libs.versions.toml" => Some(Kind::VersionCatalog),
        "yarn.lock" => Some(Kind::YarnLock),
        "pnpm-lock.yaml" => Some(Kind::PnpmLock),
        "Cargo.lock" => Some(Kind::CargoLock),
        "poetry.lock" => Some(Kind::PoetryLock),
        "requirements.txt" => Some(Kind::Requirements),
        _ => None,
    }
}

pub fn lockfile(path: &str) -> Option<String> {
    let sibling = match kind(path)? {
        Kind::PackageJson => "package-lock.json",
        Kind::ComposerJson => "composer.lock",
        _ => return None,
    };

    Some(format!("{}{sibling}", &path[..path.len() - file_name(path).len()]))
}

pub fn parse(path: &str, text: &str, lock: Option<&str>) -> Vec<Dep> {
    match kind(path) {
        Some(Kind::Pom) => pom(path, text),
        Some(Kind::Gradle) => gradle(path, text),
        Some(Kind::PackageJson) => package_json(path, text, lock),
        Some(Kind::PackageLock) => package_lock(path, text),
        Some(Kind::ComposerJson) => composer_json(path, text, lock),
        Some(Kind::ComposerLock) => composer_lock(path, text),
        Some(Kind::VersionCatalog) => version_catalog(path, text),
        Some(Kind::YarnLock) => yarn_lock(path, text),
        Some(Kind::PnpmLock) => pnpm_lock(path, text),
        Some(Kind::CargoLock) => toml_lock(path, text, Ecosystem::CratesIo, true),
        Some(Kind::PoetryLock) => toml_lock(path, text, Ecosystem::PyPI, false),
        Some(Kind::Requirements) => requirements(path, text),
        None => Vec::new(),
    }
}

/// Trimmed text of a closed element and its byte range.
fn inner_text<'a>(text: &'a str, el: &Element) -> Option<(&'a str, (usize, usize))> {
    let body = text.get(el.tag_end..el.end)?;
    if !body.ends_with('>') {
        return None;
    }
    let close = body.rfind("</")?;
    let raw = &body[..close];
    let trimmed = raw.trim();
    let from = el.tag_end + (raw.len() - raw.trim_start().len());

    Some((trimmed, (from, from + trimmed.len())))
}

fn pom(path: &str, text: &str) -> Vec<Dep> {
    let els = elements(text);
    let properties: Vec<(&str, &str, (usize, usize))> = els
        .iter()
        .filter(|e| e.parent.is_some_and(|p| els[p].name == "properties"))
        .filter_map(|e| inner_text(text, e).map(|(value, range)| (e.name.as_str(), value, range)))
        .collect();

    let mut children: HashMap<usize, [Option<usize>; 3]> = HashMap::new();
    for (child, el) in els.iter().enumerate() {
        let Some(parent) = el.parent else {
            continue;
        };
        let Some(slot) = ["groupId", "artifactId", "version"].iter().position(|n| *n == el.name) else {
            continue;
        };

        children.entry(parent).or_default()[slot].get_or_insert(child);
    }

    let mut deps = Vec::new();
    for (index, dependency) in els.iter().enumerate() {
        if dependency.name != "dependency" {
            continue;
        }
        let read = |slot: usize| {
            let child = children.get(&index)?[slot]?;

            inner_text(text, &els[child])
        };
        let (Some((group, _)), Some((artifact, at)), Some((declared, declared_at))) =
            (read(0), read(1), read(2))
        else {
            continue;
        };

        let (version, version_at) = match declared.strip_prefix("${").and_then(|d| d.strip_suffix('}')) {
            Some(key) => match properties.iter().find(|(name, _, _)| *name == key) {
                Some((_, value, range)) => (*value, *range),
                None => continue,
            },
            None => (declared, declared_at),
        };
        if !version.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }

        deps.push(Dep {
            path: path.to_owned(),
            ecosystem: Ecosystem::Maven,
            name: format!("{group}:{artifact}"),
            version: version.to_owned(),
            at,
            version_at: Some(version_at),
        });
    }

    deps
}

fn gradle(path: &str, text: &str) -> Vec<Dep> {
    let is_name = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c));

    let mut deps = Vec::new();
    let mut line_start = 0;
    for line in text.split_inclusive('\n') {
        let mut at = 0;
        while let Some(open) = line[at..].find(['\'', '"']).map(|i| at + i) {
            let quote = &line[open..=open];
            let Some(len) = line[open + 1..].find(quote) else {
                break;
            };
            let (from, to) = (open + 1, open + 1 + len);
            at = to + 1;

            let parts: Vec<&str> = line[from..to].split(':').collect();
            let [group, artifact, version] = parts[..] else {
                continue;
            };
            if !is_name(group) || !is_name(artifact) || !version.starts_with(|c: char| c.is_ascii_digit()) || version.contains('$') {
                continue;
            }

            deps.push(Dep {
                path: path.to_owned(),
                ecosystem: Ecosystem::Maven,
                name: format!("{group}:{artifact}"),
                version: version.to_owned(),
                at: (line_start + from, line_start + to),
                version_at: Some((line_start + to - version.len(), line_start + to)),
            });
        }
        line_start += line.len();
    }

    deps
}

/// The spec's leading operator (`^`, `~`, `>=`, `v`) followed by `fix`.
pub fn bumped(spec: &str, fix: &str) -> String {
    let operator = spec.len() - spec.trim_start_matches(|c: char| !c.is_ascii_digit()).len();

    format!("{}{fix}", &spec[..operator])
}

fn is_exact(spec: &str) -> bool {
    let (release, tag) = spec.split_at(spec.find(['-', '+']).unwrap_or(spec.len()));

    release.starts_with(|c: char| c.is_ascii_digit())
        && release.chars().all(|c| c.is_ascii_digit() || c == '.')
        && tag.chars().all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c))
}

/// Offset just after the `:` that follows `pos`, skipping whitespace.
fn after_colon(text: &str, pos: usize) -> Option<usize> {
    let rest = text.get(pos..)?.trim_start().strip_prefix(':')?;

    Some(text.len() - rest.trim_start().len())
}

/// Byte ranges (key contents, value contents) of the first `"key"\s*:\s*"value"`.
fn find_entry(text: &str, key: &str, value: &str) -> Option<((usize, usize), (usize, usize))> {
    let needle = format!("\"{key}\"");
    let mut from = 0;
    while let Some(found) = text[from..].find(&needle) {
        let key_start = from + found;
        from = key_start + needle.len();

        let Some(value_start) = after_colon(text, from).filter(|&at| text[at..].starts_with('"')).map(|at| at + 1) else {
            continue;
        };
        let rest = &text[value_start..];
        if rest.starts_with(value) && rest[value.len()..].starts_with('"') {
            return Some(((key_start + 1, key_start + 1 + key.len()), (value_start, value_start + value.len())));
        }
    }

    None
}

/// Offset of the `"key":` that opens a section; entries are searched from there so the same name in another section cannot match.
fn section_start(text: &str, key: &str) -> Option<usize> {
    let needle = format!("\"{key}\"");
    let mut from = 0;
    while let Some(found) = text[from..].find(&needle) {
        let at = from + found;
        from = at + needle.len();
        if after_colon(text, from).is_some() {
            return Some(at);
        }
    }

    None
}

fn shift(range: (usize, usize), base: usize) -> (usize, usize) {
    (base + range.0, base + range.1)
}

/// `(name, spec, section offset)` of every string entry in the object `section`.
fn section_entries<'a>(root: &'a Value, text: &str, section: &str) -> Vec<(&'a str, &'a str, usize)> {
    let (Some(entries), Some(base)) = (root.get(section).and_then(Value::as_object), section_start(text, section)) else {
        return Vec::new();
    };

    entries.iter().filter_map(|(name, spec)| Some((name.as_str(), spec.as_str()?, base))).collect()
}

fn package_json(path: &str, text: &str, lock: Option<&str>) -> Vec<Dep> {
    let Ok(root) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let lock: Option<Value> = lock.and_then(|l| serde_json::from_str(l).ok());

    let mut deps = Vec::new();
    for section in ["dependencies", "devDependencies"] {
        for (name, spec, base) in section_entries(&root, text, section) {
            if spec.contains(':') {
                continue;
            }
            let locked = lock
                .as_ref()
                .and_then(|l| l.get("packages")?.get(format!("node_modules/{name}"))?.get("version")?.as_str());
            let version = match locked {
                Some(version) => version,
                None if is_exact(spec) => spec,
                None => continue,
            };
            let Some((at, version_at)) = find_entry(&text[base..], name, spec) else {
                continue;
            };

            deps.push(Dep {
                path: path.to_owned(),
                ecosystem: Ecosystem::Npm,
                name: name.to_owned(),
                version: version.to_owned(),
                at: shift(at, base),
                version_at: Some(shift(version_at, base)),
            });
        }
    }

    deps
}

/// Offset of every object key after `from`, first occurrence wins; one pass keeps thousands of lock entries linear.
fn key_offsets(text: &str, from: usize) -> HashMap<&str, usize> {
    let bytes = text.as_bytes();
    let mut keys = HashMap::new();
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }
        let start = i + 1;
        i = start;
        while i < bytes.len() && bytes[i] != b'"' {
            i += if bytes[i] == b'\\' { 2 } else { 1 };
        }
        let Some(content) = text.get(start..i).filter(|_| i < bytes.len()) else {
            break;
        };
        i += 1;
        if after_colon(text, i).is_some() {
            keys.entry(content).or_insert(start);
        }
    }

    keys
}

fn package_lock(path: &str, text: &str) -> Vec<Dep> {
    let Ok(root) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let (Some(packages), Some(base)) = (root.get("packages").and_then(Value::as_object), section_start(text, "packages")) else {
        return Vec::new();
    };
    let offsets = key_offsets(text, base);

    let mut deps = Vec::new();
    for (key, entry) in packages {
        let Some(installed) = key.rsplit("node_modules/").next().filter(|_| key.starts_with("node_modules/")) else {
            continue;
        };
        if entry.get("link").is_some() {
            continue;
        }
        let (Some(version), Some(&start)) = (entry.get("version").and_then(Value::as_str), offsets.get(key.as_str())) else {
            continue;
        };

        deps.push(Dep {
            path: path.to_owned(),
            ecosystem: Ecosystem::Npm,
            name: installed.to_owned(),
            version: version.to_owned(),
            at: (start, start + key.len()),
            version_at: None,
        });
    }

    deps
}

fn locked_composer_version(entry: &Value) -> Option<&str> {
    let version = entry.get("version")?.as_str()?;
    let version = version.strip_prefix('v').unwrap_or(version);

    version.starts_with(|c: char| c.is_ascii_digit()).then_some(version)
}

fn composer_json(path: &str, text: &str, lock: Option<&str>) -> Vec<Dep> {
    let Ok(root) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let lock: Option<Value> = lock.and_then(|l| serde_json::from_str(l).ok());
    let locked = |name: &str| {
        let lock = lock.as_ref()?;
        ["packages", "packages-dev"]
            .into_iter()
            .filter_map(|section| lock.get(section)?.as_array())
            .flatten()
            .find(|entry| entry.get("name").and_then(Value::as_str) == Some(name))
            .and_then(locked_composer_version)
    };

    let mut deps = Vec::new();
    for section in ["require", "require-dev"] {
        for (name, spec, base) in section_entries(&root, text, section) {
            if !name.contains('/') {
                continue;
            }
            let version = match locked(name) {
                Some(version) => version,
                None if is_exact(spec) => spec,
                None => continue,
            };
            let Some((at, version_at)) = find_entry(&text[base..], name, spec) else {
                continue;
            };

            deps.push(Dep {
                path: path.to_owned(),
                ecosystem: Ecosystem::Packagist,
                name: name.to_owned(),
                version: version.to_owned(),
                at: shift(at, base),
                version_at: Some(shift(version_at, base)),
            });
        }
    }

    deps
}

fn composer_lock(path: &str, text: &str) -> Vec<Dep> {
    let Ok(root) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };

    let mut deps = Vec::new();
    for section in ["packages", "packages-dev"] {
        let (Some(entries), Some(mut cursor)) = (root.get(section).and_then(Value::as_array), section_start(text, section)) else {
            continue;
        };
        for entry in entries {
            let (Some(name), Some(version)) = (entry.get("name").and_then(Value::as_str), locked_composer_version(entry)) else {
                continue;
            };
            // Entries are in document order, so resuming after the previous hit keeps this linear.
            let Some((_, name_at)) = find_entry(&text[cursor..], "name", name) else {
                continue;
            };
            let at = shift(name_at, cursor);
            cursor = at.1;

            deps.push(Dep {
                path: path.to_owned(),
                ecosystem: Ecosystem::Packagist,
                name: name.to_owned(),
                version: version.to_owned(),
                at,
                version_at: None,
            });
        }
    }

    deps
}

/// `(offset, line without its line break)` for every line.
fn lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut offset = 0;

    text.split_inclusive('\n').map(move |raw| {
        let start = offset;
        offset += raw.len();

        (start, raw.trim_end_matches(['\n', '\r']))
    })
}

fn lock_dep(path: &str, ecosystem: Ecosystem, name: &str, version: &str, at: usize) -> Dep {
    Dep {
        path: path.to_owned(),
        ecosystem,
        name: name.to_owned(),
        version: version.to_owned(),
        at: (at, at + name.len()),
        version_at: None,
    }
}

fn is_version(version: &str) -> bool {
    version.starts_with(|c: char| c.is_ascii_digit())
}

/// Contents of the leading `"..."` or `'...'` of `text` and the offset of the contents within it.
fn quoted(text: &str) -> Option<(&str, usize)> {
    let quote = text.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &text[1..];

    Some((&rest[..rest.find(quote)?], 1))
}

/// Contents and absolute offset of the string in `key = "value"` found in `line`.
fn toml_string<'a>(line: &'a str, line_at: usize, key: &str) -> Option<(&'a str, usize)> {
    let mut from = 0;
    while let Some(found) = line[from..].find(key) {
        let start = from + found;
        from = start + key.len();

        let boundary = line[..start].chars().next_back().is_none_or(|c| !(c.is_ascii_alphanumeric() || "_-".contains(c)));
        let Some(rest) = line[from..].trim_start().strip_prefix('=').filter(|_| boundary) else {
            continue;
        };
        let rest = rest.trim_start();
        let (value, inside) = quoted(rest)?;

        return Some((value, line_at + line.len() - rest.len() + inside));
    }

    None
}

fn version_catalog(path: &str, text: &str) -> Vec<Dep> {
    let mut versions: HashMap<&str, &str> = HashMap::new();
    let mut section = "";
    let mut libraries: Vec<(usize, &str)> = Vec::new();
    for (at, line) in lines(text) {
        let trimmed = line.trim();
        if let Some(header) = trimmed.strip_prefix('[') {
            section = header.trim_end_matches(']').trim();

            continue;
        }
        match section {
            "versions" => {
                if let Some((key, value)) = trimmed.split_once('=')
                    && let Some((version, _)) = quoted(value.trim_start())
                {
                    versions.insert(key.trim(), version);
                }
            }
            "libraries" if !trimmed.starts_with('#') => libraries.push((at, line)),
            _ => {}
        }
    }

    let mut deps = Vec::new();
    for (at, line) in libraries {
        let Some((_, value)) = line.split_once('=') else {
            continue;
        };
        let value_at = at + line.len() - value.trim_start().len();
        let entry = |key: &str| toml_string(line, at, key);

        let (name, name_at, literal) = match (entry("module"), entry("group"), entry("name")) {
            (Some((module, module_at)), ..) => (module.to_owned(), module_at, None),
            (None, Some((group, group_at)), Some((artifact, _))) => (format!("{group}:{artifact}"), group_at, None),
            _ => match quoted(value.trim_start()) {
                Some((coordinates, inside)) => {
                    let parts: Vec<&str> = coordinates.split(':').collect();
                    let [group, artifact, version] = parts[..] else {
                        continue;
                    };

                    (format!("{group}:{artifact}"), value_at + inside, Some(version))
                }
                None => continue,
            },
        };
        let version = literal
            .or_else(|| entry("version").map(|(version, _)| version))
            .or_else(|| entry("version.ref").or_else(|| entry("ref")).and_then(|(key, _)| versions.get(key).copied()));
        let Some(version) = version.filter(|v| is_version(v)) else {
            continue;
        };

        let mut dep = lock_dep(path, Ecosystem::Maven, &name, version, name_at);
        dep.at.1 = name_at + name.len().max(1);
        deps.push(dep);
    }

    deps
}

fn yarn_lock(path: &str, text: &str) -> Vec<Dep> {
    let mut deps = Vec::new();
    let mut header: Option<(&str, usize)> = None;
    for (at, line) in lines(text) {
        if !line.starts_with([' ', '#']) && line.ends_with(':') {
            header = line.split(',').next().and_then(|first| {
                let first = first.trim_end_matches(':');
                let (spec, inside) = quoted(first).unwrap_or((first, 0));
                let split = spec.rfind('@').filter(|&i| i > 0)?;
                let range = &spec[split + 1..];
                if range.contains(':') && !range.starts_with("npm:") {
                    return None;
                }

                Some((&spec[..split], at + inside))
            });

            continue;
        }

        let Some(rest) = line.strip_prefix("  version") else {
            continue;
        };
        let Some((name, name_at)) = header else {
            continue;
        };
        let version = rest.trim_start_matches(':').trim().trim_matches('"');
        if is_version(version) {
            deps.push(lock_dep(path, Ecosystem::Npm, name, version, name_at));
        }
    }

    deps
}

fn pnpm_lock(path: &str, text: &str) -> Vec<Dep> {
    let mut deps = Vec::new();
    let mut in_packages = false;
    for (at, line) in lines(text) {
        if !line.starts_with([' ', '#']) && !line.is_empty() {
            in_packages = line.trim_end() == "packages:";

            continue;
        }
        if !in_packages || line.starts_with("   ") {
            continue;
        }
        let Some(key) = line.strip_prefix("  ").and_then(|k| k.strip_suffix(':')) else {
            continue;
        };
        let (key, inside) = quoted(key).unwrap_or((key, 0));
        let slash = usize::from(key.starts_with('/'));
        let key = key[slash..].split('(').next().unwrap_or_default();
        let Some(split) = key.rfind('@').filter(|&i| i > 0) else {
            continue;
        };
        let version = &key[split + 1..];
        if is_version(version) {
            deps.push(lock_dep(path, Ecosystem::Npm, &key[..split], version, at + 2 + inside + slash));
        }
    }

    deps
}

/// `[[package]]` tables of `Cargo.lock` and `poetry.lock`; Cargo members and path or git packages have no registry source.
fn toml_lock(path: &str, text: &str, ecosystem: Ecosystem, registry_only: bool) -> Vec<Dep> {
    let mut deps = Vec::new();
    let mut current: Option<(&str, usize, &str, bool)> = None;
    let mut flush = |current: &mut Option<(&str, usize, &str, bool)>| {
        if let Some((name, at, version, sourced)) = current.take()
            && !name.is_empty()
            && is_version(version)
            && (sourced || !registry_only)
        {
            deps.push(lock_dep(path, ecosystem, name, version, at));
        }
    };
    for (at, line) in lines(text) {
        if line.trim() == "[[package]]" {
            flush(&mut current);
            current = Some(("", 0, "", false));

            continue;
        }
        let Some(package) = current.as_mut() else {
            continue;
        };
        if let Some((name, name_at)) = toml_string(line, at, "name") {
            (package.0, package.1) = (name, name_at);
        } else if let Some((version, _)) = toml_string(line, at, "version") {
            package.2 = version;
        } else if let Some((source, _)) = toml_string(line, at, "source") {
            package.3 = source.starts_with("registry+");
        }
    }
    flush(&mut current);

    deps
}

fn requirements(path: &str, text: &str) -> Vec<Dep> {
    let mut deps = Vec::new();
    for (at, line) in lines(text) {
        let spec = line.split(['#', ';']).next().unwrap_or_default();
        let Some((name, version)) = spec.split_once("==") else {
            continue;
        };
        let name = name.trim();
        let name = name.split('[').next().unwrap_or(name);
        let version = version.split_whitespace().next().unwrap_or_default();
        let is_name = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c));
        if is_name && is_version(version) && !version.contains('*') {
            deps.push(lock_dep(path, Ecosystem::PyPI, name, version, at + line.len() - line.trim_start().len()));
        }
    }

    deps
}

#[cfg(test)]
mod tests {
    use super::*;

    const POM: &str = r#"<project>
  <properties><jackson.version>2.9.10</jackson.version></properties>
  <dependencies>
    <dependency><groupId>com.fasterxml.jackson.core</groupId><artifactId>jackson-databind</artifactId><version>${jackson.version}</version></dependency>
    <dependency><groupId>org.yaml</groupId><artifactId>snakeyaml</artifactId><version> 1.33 </version></dependency>
    <dependency><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter-web</artifactId></dependency>
    <dependency><groupId>x</groupId><artifactId>y</artifactId><version>${missing}</version></dependency>
    <dependency><groupId>x</groupId><artifactId>z</artifactId><version>[1.0,2.0)</version></dependency>
  </dependencies>
  <build><plugins><plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-jar-plugin</artifactId><version>3.0.0</version></plugin></plugins></build>
</project>"#;

    fn found(deps: &[Dep]) -> Vec<(&str, &str)> {
        deps.iter().map(|d| (d.name.as_str(), d.version.as_str())).collect()
    }

    #[test]
    fn pom_reads_versions_and_properties() {
        let deps = parse("/p/pom.xml", POM, None);

        assert_eq!(found(&deps), [("com.fasterxml.jackson.core:jackson-databind", "2.9.10"), ("org.yaml:snakeyaml", "1.33")]);
        assert_eq!(deps[0].ecosystem, Ecosystem::Maven);
        assert_eq!(deps[0].path, "/p/pom.xml");
        let (from, to) = deps[0].version_at.unwrap();
        assert_eq!(&POM[from..to], "2.9.10");
        let (from, to) = deps[1].version_at.unwrap();
        assert_eq!(&POM[from..to], "1.33");
        assert_eq!(&POM[deps[1].at.0..deps[1].at.1], "snakeyaml");
    }

    #[test]
    fn kinds_and_lockfiles_by_file_name() {
        assert_eq!(kind("/p/api/build.gradle.kts"), Some(Kind::Gradle));
        assert_eq!(kind("/p/composer.lock"), Some(Kind::ComposerLock));
        assert_eq!(kind("/p/foopom.xml"), None);
        assert_eq!(lockfile("/p/web/package.json").as_deref(), Some("/p/web/package-lock.json"));
        assert_eq!(lockfile("/p/pom.xml"), None);
    }

    #[test]
    fn pom_never_panics_on_half_typed_text() {
        let text = format!("{POM}é");
        for end in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
            parse("/p/pom.xml", &text[..end], None);
        }
    }

    const GRADLE: &str = "plugins {\n    id 'org.springframework.boot' version '3.1.0'\n}\ndependencies {\n    implementation 'org.yaml:snakeyaml:1.33'\n    implementation(\"com.google.guava:guava:31.0-jre\")\n    implementation \"org.x:y:$v\"\n    implementation 'org.springframework.boot:spring-boot-starter'\n}\n";

    #[test]
    fn gradle_reads_string_coordinates() {
        let deps = parse("/p/build.gradle.kts", GRADLE, None);

        assert_eq!(found(&deps), [("org.yaml:snakeyaml", "1.33"), ("com.google.guava:guava", "31.0-jre")]);
        let (from, to) = deps[1].version_at.unwrap();
        assert_eq!(&GRADLE[from..to], "31.0-jre");
        assert_eq!(&GRADLE[deps[0].at.0..deps[0].at.1], "org.yaml:snakeyaml:1.33");
    }

    #[test]
    fn gradle_never_panics_on_half_typed_text() {
        let text = format!("{GRADLE}implementation 'é:é:1' \"é");
        for end in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
            parse("/p/build.gradle", &text[..end], None);
        }
    }

    const PACKAGE: &str = r#"{"dependencies": {"lodash": "^4.17.15", "vue": "3.2.0", "local": "file:../x"},
 "devDependencies": {"@babel/traverse": "~7.22.0"}}"#;
    const PACKAGE_LOCK: &str = r#"{"lockfileVersion": 3, "packages": {"": {"name": "app"},
 "node_modules/lodash": {"version": "4.17.15"}, "node_modules/@babel/traverse": {"version": "7.22.5"},
 "node_modules/foo/node_modules/minimist": {"version": "1.2.0"}, "node_modules/linked": {"link": true}}}"#;
    const COMPOSER: &str = r#"{"require": {"php": ">=8.1", "ext-json": "*", "symfony/http-kernel": "^5.4", "guzzlehttp/guzzle": "7.4.0"},
 "require-dev": {"phpunit/phpunit": "^9.5"}}"#;
    const COMPOSER_LOCK: &str = r#"{"packages": [{"name": "symfony/http-kernel", "version": "v5.4.20"}, {"name": "guzzlehttp/guzzle", "version": "7.4.0"}],
 "packages-dev": [{"name": "phpunit/phpunit", "version": "dev-main"}]}"#;

    fn sorted(deps: &[Dep]) -> Vec<(&str, &str)> {
        let mut all = found(deps);
        all.sort();
        all
    }

    fn named<'a>(deps: &'a [Dep], name: &str) -> &'a Dep {
        deps.iter().find(|d| d.name == name).unwrap()
    }

    #[test]
    fn package_json_takes_locked_versions() {
        let deps = parse("/p/package.json", PACKAGE, Some(PACKAGE_LOCK));

        assert_eq!(sorted(&deps), [("@babel/traverse", "7.22.5"), ("lodash", "4.17.15"), ("vue", "3.2.0")]);
        let lodash = named(&deps, "lodash");
        assert_eq!(lodash.ecosystem, Ecosystem::Npm);
        assert_eq!(&PACKAGE[lodash.at.0..lodash.at.1], "lodash");
        let (from, to) = lodash.version_at.unwrap();
        assert_eq!(&PACKAGE[from..to], "^4.17.15");
    }

    #[test]
    fn package_json_without_lockfile_keeps_exact_versions_only() {
        assert_eq!(found(&parse("/p/package.json", PACKAGE, None)), [("vue", "3.2.0")]);
    }

    #[test]
    fn package_lock_lists_every_installed_package() {
        let deps = parse("/p/package-lock.json", PACKAGE_LOCK, None);

        assert_eq!(sorted(&deps), [("@babel/traverse", "7.22.5"), ("lodash", "4.17.15"), ("minimist", "1.2.0")]);
        assert!(deps.iter().all(|d| d.version_at.is_none()));
        let lodash = named(&deps, "lodash");
        assert_eq!(&PACKAGE_LOCK[lodash.at.0..lodash.at.1], "node_modules/lodash");
    }

    #[test]
    fn composer_json_takes_locked_versions_without_v_prefix() {
        let deps = parse("/p/composer.json", COMPOSER, Some(COMPOSER_LOCK));

        assert_eq!(sorted(&deps), [("guzzlehttp/guzzle", "7.4.0"), ("symfony/http-kernel", "5.4.20")]);
        assert_eq!(deps[0].ecosystem, Ecosystem::Packagist);
        assert_eq!(found(&parse("/p/composer.json", COMPOSER, None)), [("guzzlehttp/guzzle", "7.4.0")]);
    }

    #[test]
    fn composer_lock_lists_every_package() {
        let deps = parse("/p/composer.lock", COMPOSER_LOCK, None);

        assert_eq!(sorted(&deps), [("guzzlehttp/guzzle", "7.4.0"), ("symfony/http-kernel", "5.4.20")]);
        let kernel = named(&deps, "symfony/http-kernel");
        assert_eq!(&COMPOSER_LOCK[kernel.at.0..kernel.at.1], "symfony/http-kernel");
    }

    #[test]
    fn bumped_keeps_the_constraint_operator() {
        assert_eq!(bumped("^4.17.15", "4.17.21"), "^4.17.21");
        assert_eq!(bumped("2.12.3", "2.12.7.1"), "2.12.7.1");
    }

    #[test]
    fn offsets_point_into_the_section_they_came_from() {
        let text = r#"{"dependencies": {"lodash": "4.0.0"}, "devDependencies": {"lodash": "4.1.0"}}"#;
        let deps = parse("/p/package.json", text, None);

        assert_eq!(deps.len(), 2);
        for dep in &deps {
            let (from, to) = dep.version_at.unwrap();
            assert_eq!(&text[from..to], dep.version);
            assert_eq!(text[..from].matches("lodash").count(), if dep.version == "4.0.0" { 1 } else { 2 });
        }
    }

    #[test]
    fn nested_lock_keys_resolve_to_their_own_entry() {
        let text = r#"{"packages": {"node_modules/a/node_modules/x": {"version": "1.0.0"}, "node_modules/x": {"version": "2.0.0"}}}"#;
        let deps = parse("/p/package-lock.json", text, None);

        assert_eq!(deps.len(), 2);
        let nested = deps.iter().find(|d| d.version == "1.0.0").unwrap();
        assert_eq!(&text[nested.at.0..nested.at.1], "node_modules/a/node_modules/x");
        let top = deps.iter().find(|d| d.version == "2.0.0").unwrap();
        assert_eq!(&text[top.at.0..top.at.1], "node_modules/x");
    }

    #[test]
    fn json_never_panics_on_half_typed_text() {
        for (path, text) in [
            ("/p/package.json", PACKAGE),
            ("/p/package-lock.json", PACKAGE_LOCK),
            ("/p/composer.json", COMPOSER),
            ("/p/composer.lock", COMPOSER_LOCK),
        ] {
            let text = format!("{text}é\"é");
            for end in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
                parse(path, &text[..end], Some(COMPOSER_LOCK));
            }
        }
    }

    #[test]
    fn reads_libs_versions_toml_aliases() {
        let text = "[versions]\njackson = \"2.12.3\"\n[libraries]\na = { module = \"com.fasterxml.jackson.core:jackson-databind\", version.ref = \"jackson\" }\nb = { group = \"org.yaml\", name = \"snakeyaml\", version = \"1.33\" }\nc = \"com.google.guava:guava:31.0-jre\"\nd = { module = \"x:managed\" }\n# old = { module = \"x:old\", version = \"1.0.0\" }\n  #g = \"x:older:2.0.0\"\ne = { module = \"x:dangling\", version = { ref = \"nope\" } }\nf = { module = \"x:ref\", version = { ref = \"jackson\" } }\n[plugins]\np = { id = \"x\", version = \"1.0\" }\n";
        let deps = parse("/p/gradle/libs.versions.toml", text, None);

        assert_eq!(
            found(&deps),
            [
                ("com.fasterxml.jackson.core:jackson-databind", "2.12.3"),
                ("org.yaml:snakeyaml", "1.33"),
                ("com.google.guava:guava", "31.0-jre"),
                ("x:ref", "2.12.3")
            ]
        );
        assert_eq!(deps[0].ecosystem, Ecosystem::Maven);
        assert_eq!(&text[deps[0].at.0..deps[0].at.1], "com.fasterxml.jackson.core:jackson-databind");
        assert_eq!(&text[deps[2].at.0..deps[2].at.1], "com.google.guava:guava");
    }

    #[test]
    fn reads_yarn_lock_v1_and_berry() {
        let v1 = "# yarn lockfile v1\n\n\"@babel/core@^7.0.0\", \"@babel/core@^7.1.0\":\n  version \"7.1.2\"\n  resolved \"x\"\n\nlodash@^4.17.15:\n  version \"4.17.15\"\n\nlocal@file:../x:\n  version \"1.0.0\"\n";
        let deps = parse("/p/yarn.lock", v1, None);
        assert_eq!(found(&deps), [("@babel/core", "7.1.2"), ("lodash", "4.17.15")]);
        assert_eq!(&v1[deps[1].at.0..deps[1].at.1], "lodash");
        assert_eq!(&v1[deps[0].at.0..deps[0].at.1], "@babel/core");

        let berry = "__metadata:\n  version: 6\n\n\"lodash@npm:^4.17.15\":\n  version: 4.17.21\n  resolution: \"lodash@npm:4.17.21\"\n\n\"app@workspace:.\":\n  version: 0.0.0-use.local\n";
        let deps = parse("/p/yarn.lock", berry, None);
        assert_eq!(found(&deps), [("lodash", "4.17.21")]);
        assert_eq!(&berry[deps[0].at.0..deps[0].at.1], "lodash");
    }

    #[test]
    fn reads_pnpm_lock() {
        let text = "lockfileVersion: '6.0'\n\nimporters:\n  .:\n    dependencies:\n      lodash:\n        version: 4.17.15\n\npackages:\n\n  /lodash@4.17.15:\n    resolution: {integrity: x}\n\n  /@babel/traverse@7.22.5(supports-color@5.5.0):\n    dev: true\n\n  '@scope/pkg@1.0.0':\n    dev: false\n\n  vue@3.2.0:\n    dev: false\n\nsnapshots:\n  other@9.9.9:\n    dev: false\n";
        let deps = parse("/p/pnpm-lock.yaml", text, None);

        assert_eq!(found(&deps), [("lodash", "4.17.15"), ("@babel/traverse", "7.22.5"), ("@scope/pkg", "1.0.0"), ("vue", "3.2.0")]);
        for dep in &deps {
            assert_eq!(&text[dep.at.0..dep.at.1], dep.name);
        }
    }

    #[test]
    fn reads_cargo_lock() {
        let text = "version = 3\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.1\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n\n[[package]]\nname = \"local\"\nversion = \"0.2.0\"\nsource = \"path+file:///x\"\n";
        let deps = parse("/p/Cargo.lock", text, None);

        assert_eq!(found(&deps), [("serde", "1.0.1")]);
        assert_eq!(deps[0].ecosystem, Ecosystem::CratesIo);
        assert_eq!(&text[deps[0].at.0..deps[0].at.1], "serde");
    }

    #[test]
    fn reads_pypi_pins_and_poetry_lock() {
        let reqs = "# deps\nDjango==3.2.0  # pinned\nrequests[security]==2.0.0 ; python_version > '3'\nflask>=1.0\n-r other.txt\n";
        let deps = parse("/p/requirements.txt", reqs, None);
        assert_eq!(found(&deps), [("Django", "3.2.0"), ("requests", "2.0.0")]);
        assert_eq!(&reqs[deps[0].at.0..deps[0].at.1], "Django");
        assert_eq!(deps[0].ecosystem, Ecosystem::PyPI);

        let poetry = "[[package]]\nname = \"django\"\nversion = \"3.2.0\"\n\n[metadata]\nlock-version = \"2.0\"\n";
        assert_eq!(found(&parse("/p/poetry.lock", poetry, None)), [("django", "3.2.0")]);
    }

    #[test]
    fn new_formats_never_panic_on_half_typed_text() {
        for (path, text) in [
            ("/p/libs.versions.toml", "[versions]\nj = \"é\"\n[libraries]\na = { module = \"é:é\", version.ref = \"j\" }\nb = \"é:é:1\"\n"),
            ("/p/yarn.lock", "\"é@npm:^1\", \"é@^1\":\n  version: 1.0.0\n"),
            ("/p/pnpm-lock.yaml", "packages:\n  '/é@1.0.0(é@1)':\n    dev: true\n"),
            ("/p/Cargo.lock", "[[package]]\nname = \"é\"\nversion = \"1.0.0\"\nsource = \"registry+é\"\n"),
            ("/p/requirements.txt", "é[é]==1.0 ; é # é\n"),
        ] {
            let text = format!("{text}é\"é'");
            for end in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
                parse(path, &text[..end], None);
            }
        }
    }
}
