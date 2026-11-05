//! Dependencies declared by the project manifests and lockfiles.

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

pub fn parse(path: &str, text: &str, _lock: Option<&str>) -> Vec<Dep> {
    match kind(path) {
        Some(Kind::Pom) => pom(path, text),
        Some(Kind::Gradle) => gradle(path, text),
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
}
