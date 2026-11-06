//! What a project declares and reads: keys of `.env` files, Dockerfiles and
//! docker-compose files, and where code (or `${KEY}`) uses them. The original
//! plugin keeps this in IntelliJ's file index; here a project walk builds it
//! and saved files update it.

use crate::{code, docker, parse};
use openidea_plugin::{Location, Project, Range, text};
use std::collections::BTreeSet;
use std::time::Instant;

/// Env files read in each folder that looks like a project: the walk
/// (`find_files`) skips hidden files, so `.env*` can't be listed.
pub const ENV_FILES: &[&str] = &[
    ".env",
    ".env.local",
    ".env.example",
    ".env.dist",
    ".env.sample",
    ".env.template",
    ".env.defaults",
    ".env.dev",
    ".env.development",
    ".env.development.local",
    ".env.test",
    ".env.test.local",
    ".env.testing",
    ".env.staging",
    ".env.prod",
    ".env.production",
    ".env.production.local",
    ".env.ci",
    ".env.docker",
    ".flaskenv",
];

/// Files whose folder gets [`ENV_FILES`] looked up, besides the root.
// ponytail: `.env` elsewhere is only known once opened; the host can't list hidden files.
const MARKERS: &[&str] = &[
    "package.json",
    "composer.json",
    "pyproject.toml",
    "requirements.txt",
    "manage.py",
    "go.mod",
    "Gemfile",
    "Cargo.toml",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "artisan",
    "Dockerfile",
    "docker-compose.yml",
    "docker-compose.yaml",
    "compose.yml",
    "compose.yaml",
];

const CODE: &[&str] = &[
    "php", "py", "js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts", "vue", "svelte", "go", "rb", "rake", "rs", "java",
    "kt", "kts", "scala", "groovy", "cs",
];

/// Paths the walk returns at most.
const MAX_FILES: u32 = 50_000;

#[derive(Clone, Debug)]
pub struct Declaration {
    pub key: String,
    pub value: String,
    pub location: Location,
    /// From `.env`, a Dockerfile or a compose file; `.env.example` and the
    /// like only count for a key when no primary file has it.
    pub primary: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Usage {
    pub key: String,
    pub location: Location,
}

#[derive(Default)]
pub struct Index {
    /// Built from the whole project, not just the root's env files.
    pub complete: bool,
    pub declarations: Vec<Declaration>,
    pub usages: Vec<Usage>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    Env,
    Dockerfile,
    Compose,
    Code,
}

pub fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

pub fn is_env_file(path: &str) -> bool {
    let name = file_name(path);
    name == ".env" || name == ".flaskenv" || name.starts_with(".env.") || name.ends_with(".env")
}

fn source(path: &str) -> Option<Source> {
    let name = file_name(path);
    if is_env_file(path) {
        return Some(Source::Env);
    }
    if name == "Dockerfile" || name.starts_with("Dockerfile.") || name.to_ascii_lowercase().ends_with(".dockerfile") {
        return Some(Source::Dockerfile);
    }
    if matches!(name, "docker-compose.yml" | "docker-compose.yaml" | "compose.yml" | "compose.yaml") {
        return Some(Source::Compose);
    }
    let code = name.starts_with("phpunit.xml") || name.rsplit_once('.').is_some_and(|(_, ext)| CODE.contains(&ext));
    code.then_some(Source::Code)
}

pub fn range(text: &str, bytes: std::ops::Range<usize>) -> Range {
    Range::new(text::position(text, bytes.start), text::position(text, bytes.end))
}

pub fn env_declarations(path: &str, text: &str) -> Vec<Declaration> {
    let primary = file_name(path) == ".env";
    let tokens = parse::lex(text);
    parse::properties(text, &tokens)
        .into_iter()
        .map(|p| Declaration {
            key: p.name(text),
            value: p.short_value(),
            location: Location { path: path.to_owned(), range: range(text, p.key.clone()) },
            primary,
        })
        .collect()
}

/// `${KEY}` references of a `.env` file.
pub fn nested_usages(path: &str, text: &str) -> Vec<Usage> {
    parse::lex(text)
        .into_iter()
        .filter(|t| t.kind == parse::Kind::NestedKey)
        .map(|t| Usage {
            key: text[t.range.clone()].to_owned(),
            location: Location { path: path.to_owned(), range: range(text, t.range) },
        })
        .collect()
}

fn code_usages(path: &str, text: &str) -> Vec<Usage> {
    let mut usages = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        // Minified code: one huge line, no `getenv` worth a quadratic scan.
        if body.len() <= 2000 {
            for key in code::keys_in_line(body) {
                usages.push(Usage {
                    key: body[key.clone()].to_owned(),
                    location: Location { path: path.to_owned(), range: range(text, offset + key.start..offset + key.end) },
                });
            }
        }
        offset += line.len();
    }
    usages
}

fn docker_declarations(path: &str, text: &str, entries: Vec<docker::Entry>) -> Vec<Declaration> {
    entries
        .into_iter()
        .map(|(key, value, r)| Declaration {
            key,
            value,
            location: Location { path: path.to_owned(), range: range(text, r) },
            primary: true,
        })
        .collect()
}

fn normalise(path: &str) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts)
}

/// `file` (absolute, or relative to `folder`) with `.` and `..` resolved, or
/// `None` when it ends up outside `root`.
fn confined_path(root: &str, folder: &str, file: &str) -> Option<String> {
    let joined = if file.starts_with('/') { file.to_owned() } else { format!("{folder}/{file}") };
    let root = normalise(root)?;
    let parts = normalise(&joined)?;
    if parts.len() <= root.len() || parts[..root.len()] != root[..] {
        return None;
    }
    Some(format!("/{}", parts.join("/")))
}

impl Index {
    fn add(&mut self, path: &str, text: &str) {
        match source(path) {
            Some(Source::Env) => {
                self.declarations.extend(env_declarations(path, text));
                self.usages.extend(nested_usages(path, text));
            }
            Some(Source::Dockerfile) => self.declarations.extend(docker_declarations(path, text, docker::dockerfile(text))),
            Some(Source::Compose) => self.declarations.extend(docker_declarations(path, text, docker::compose(text))),
            Some(Source::Code) => self.usages.extend(code_usages(path, text)),
            None => {}
        }
    }

    /// Reads the files a compose file names in `env_file:` and declares their
    /// keys, unless the index already has that file. `read_file` fails for a
    /// missing file (`NotFound` or `Invalid`): then there is nothing to add.
    pub fn add_compose_env_files(&mut self, project: &Project, root: &str, compose_path: &str, text: &str) {
        if source(compose_path) != Some(Source::Compose) {
            return;
        }
        let folder = compose_path.rsplit_once(['/', '\\']).map_or("", |(folder, _)| folder);
        for file in docker::compose_env_files(text) {
            let Some(path) = confined_path(root, folder, &file) else { continue };
            if self.declarations.iter().any(|d| d.location.path == path) {
                continue;
            }
            if let Ok(env) = project.read_file(&path) {
                self.declarations.extend(env_declarations(&path, &env).into_iter().map(|d| Declaration { primary: true, ..d }));
            }
        }
    }

    /// Replaces what `path` declares and uses with `text` (`None`: it is gone).
    pub fn update(&mut self, path: &str, text: Option<&str>) {
        self.declarations.retain(|d| d.location.path != path);
        self.usages.retain(|u| u.location.path != path);
        if let Some(text) = text {
            self.add(path, text);
        }
    }

    pub fn indexes(path: &str) -> bool {
        source(path).is_some()
    }

    /// The env files at the root only: quick enough for an editor request.
    pub fn quick(project: &Project, root: &str) -> Self {
        let mut index = Self::default();
        for name in ENV_FILES {
            let path = format!("{}/{name}", root.trim_end_matches(['/', '\\']));
            if let Ok(text) = project.read_file(&path) {
                index.add(&path, &text);
            }
        }
        index
    }

    /// Every file that declares or reads keys, declarations first, until `deadline`.
    // ponytail: a huge project is cut at the deadline (usages first); persist the index if that bites.
    pub fn full(project: &Project, root: &str, deadline: Instant) -> Self {
        let files = project.find_files("**/*", MAX_FILES).unwrap_or_default();
        let mut folders = BTreeSet::from([root.trim_end_matches(['/', '\\']).to_owned()]);
        for file in &files {
            if MARKERS.contains(&file_name(file)) {
                if let Some((folder, _)) = file.rsplit_once(['/', '\\']) {
                    folders.insert(folder.to_owned());
                }
            }
        }

        let mut paths: Vec<(Source, String)> = folders
            .iter()
            .flat_map(|folder| ENV_FILES.iter().map(move |name| (Source::Env, format!("{folder}/{name}"))))
            .chain(files.into_iter().filter_map(|f| Some((source(&f)?, f))))
            .collect();
        paths.sort();
        paths.dedup();

        let mut index = Self { complete: true, ..Self::default() };
        for (_, path) in paths {
            if Instant::now() > deadline {
                break;
            }
            if let Ok(text) = project.read_file(&path) {
                index.add(&path, &text);
                index.add_compose_env_files(project, root, &path, &text);
            }
        }
        index
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::Position;
    use openidea_plugin::mock::MockHost;
    use std::time::Duration;

    #[test]
    fn indexes_the_project() {
        let host = MockHost::install();
        host.set_project_root(Some("/p"));
        host.add_file(".env", "A=1\nB=\"${A}\"\n");
        host.add_file("api/composer.json", "{}");
        host.add_file("api/.env.example", "C=3\n");
        host.add_file("Dockerfile", "ENV D=4\n");
        host.add_file("deploy/docker-compose.yml", "services:\n  x:\n    env_file: [\"./conf/vars\", missing.env]\n    environment:\n      - E=5\n");
        host.add_file("deploy/conf/vars", "F=6\n");
        host.add_file("api/index.php", "<?php\necho getenv('A') . $_ENV['C'];\n");
        host.add_file("README.md", "getenv('NOPE')");
        let project = host.invocation().project().unwrap();

        let index = Index::full(&project, "/p", Instant::now() + Duration::from_secs(5));
        let mut keys: Vec<_> = index.declarations.iter().map(|d| (d.key.as_str(), d.primary)).collect();
        keys.sort();
        assert_eq!(keys, [("A", true), ("B", true), ("C", false), ("D", true), ("E", true), ("F", true)]);
        let usages: Vec<_> = index.usages.iter().map(|u| (u.key.as_str(), file_name(&u.location.path))).collect();
        assert_eq!(usages, [("A", ".env"), ("A", "index.php"), ("C", "index.php")]);
        assert_eq!(index.usages[1].location.range, Range::new(Position::new(1, 13), Position::new(1, 14)));

        let quick = Index::quick(&project, "/p");
        assert_eq!(quick.declarations.len(), 2, "only the root's .env");
    }

    #[test]
    fn env_file_paths_are_confined_to_the_root() {
        assert_eq!(confined_path("/p", "/p/deploy", "../.env").as_deref(), Some("/p/.env"));
        assert_eq!(confined_path("/p/", "/p/deploy", "./a/../x.env").as_deref(), Some("/p/deploy/x.env"));
        assert_eq!(confined_path("/p", "/p/deploy", "/p/y.env").as_deref(), Some("/p/y.env"));
        assert_eq!(confined_path("/p", "/p/deploy", "../../outside.env"), None);
        assert_eq!(confined_path("/p", "/p/deploy", "/etc/passwd"), None);
        assert_eq!(confined_path("/p", "/p/deploy", "../../../x"), None);
    }

    fn compose_index(env_file: &str) -> Index {
        let host = MockHost::install();
        host.set_project_root(Some("/p"));
        host.add_file(".env", "A=1\n");
        host.add_file("/outside.env", "OUT=1\n");
        host.add_file("deploy/docker-compose.yml", &format!("services:\n  x:\n    env_file: {env_file}\n"));
        let project = host.invocation().project().unwrap();
        Index::full(&project, "/p", Instant::now() + Duration::from_secs(5))
    }

    #[test]
    fn env_file_parent_dir_path_is_normalised_and_deduped() {
        let index = compose_index("../.env");
        let found: Vec<_> = index.declarations.iter().map(|d| (d.key.as_str(), d.location.path.as_str())).collect();
        assert_eq!(found, [("A", "/p/.env")]);
    }

    #[test]
    fn env_file_escaping_the_root_is_ignored() {
        for path in ["../../outside.env", "/outside.env", "/p/../outside.env"] {
            let index = compose_index(path);
            assert!(index.declarations.iter().all(|d| d.key != "OUT"), "{path}");
        }
    }
}
