//! Dependencies declared by the project manifests and lockfiles.

use std::collections::HashMap;

use serde_json::Value;
use spring_text::xml::{Element, elements};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ecosystem {
    Maven,
    Npm,
    Packagist,
}

impl Ecosystem {
    pub fn osv(self) -> &'static str {
        match self {
            Ecosystem::Maven => "Maven",
            Ecosystem::Npm => "npm",
            Ecosystem::Packagist => "Packagist",
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
}

#[allow(dead_code)]
pub const GLOBS: &[&str] = &[
    "**/pom.xml",
    "**/build.gradle",
    "**/build.gradle.kts",
    "**/package.json",
    "**/composer.json",
    "**/package-lock.json",
    "**/composer.lock",
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
        _ => Vec::new(),
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

    let mut deps = Vec::new();
    for (index, dependency) in els.iter().enumerate() {
        if dependency.name != "dependency" {
            continue;
        }
        let read = |name: &str| {
            els.iter()
                .find(|e| e.parent == Some(index) && e.name == name)
                .and_then(|e| inner_text(text, e))
        };
        let (Some((group, _)), Some((artifact, at)), Some((declared, declared_at))) =
            (read("groupId"), read("artifactId"), read("version"))
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
}
