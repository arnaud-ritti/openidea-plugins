//! Per-project file indexes read a little at a time, so that every editor
//! provider call stays within its 50 ms.

use openidea_plugin::{Project, host};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const MAX_FILES: u32 = 10_000;
/// How long a full pass over the project is trusted; open documents are always re-read.
const TTL: Duration = Duration::from_secs(120);
/// Reading time per editor provider call.
pub const PROVIDER_BUDGET: Duration = Duration::from_millis(30);
/// Reading time for a command or a view.
pub const COMMAND_BUDGET: Duration = Duration::from_secs(3);

/// The project holding `path` (the innermost when projects nest).
pub fn project_of(path: &str) -> Option<Project> {
    host::projects()
        .ok()?
        .into_iter()
        .filter(|p| p.root().is_ok_and(|r| path.starts_with(&r)))
        .max_by_key(|p| p.root().map(|r| r.len()).unwrap_or(0))
}

/// What a project's files matching some globs say, by absolute path. The
/// first pass fills `files` as it goes; later passes replace it when done.
pub struct Index<T> {
    files: BTreeMap<String, T>,
    next: BTreeMap<String, T>,
    unread: Vec<String>,
    scanned: Option<Instant>,
    /// A pass has started and not finished.
    reading: bool,
    passes: u32,
    globs: &'static [&'static str],
    parse: fn(&str, &str) -> Option<T>,
}

impl<T: Clone> Index<T> {
    /// `parse(path, text)` keeps what matters of a file (`None`: nothing).
    pub fn new(globs: &'static [&'static str], parse: fn(&str, &str) -> Option<T>) -> Self {
        Self { files: BTreeMap::new(), next: BTreeMap::new(), unread: Vec::new(), scanned: None, reading: false, passes: 0, globs, parse }
    }

    /// Reads files for at most `budget`.
    pub fn scan(&mut self, project: &Project, budget: Duration) {
        if self.unread.is_empty() && self.scanned.is_none_or(|at| at.elapsed() > TTL) {
            let mut paths: Vec<String> =
                self.globs.iter().flat_map(|g| project.find_files(g, MAX_FILES).unwrap_or_default()).collect();
            paths.sort();
            paths.dedup();
            paths.reverse(); // popped from the end: read in path order
            self.unread = paths;
            self.scanned = Some(Instant::now());
            self.reading = true;
        }
        let start = Instant::now();
        let first = self.passes == 0;
        while start.elapsed() < budget
            && let Some(path) = self.unread.pop()
        {
            let Ok(text) = project.read_file(&path) else { continue };
            let target = if first { &mut self.files } else { &mut self.next };
            match (self.parse)(&path, &text) {
                Some(value) => {
                    target.insert(path, value);
                }
                None => {
                    target.remove(&path);
                }
            }
        }
        if self.reading && self.unread.is_empty() {
            if !first {
                self.files = std::mem::take(&mut self.next);
            }
            self.reading = false;
            self.passes += 1;
        }
    }

    /// Replaces what a file says with its current text (an open document).
    pub fn update(&mut self, path: &str, text: &str) {
        self.unread.retain(|p| p != path);
        self.files.remove(path);
        self.next.remove(path);
        let Some(value) = (self.parse)(path, text) else { return };
        if self.reading && self.passes > 0 {
            self.next.insert(path.to_owned(), value.clone());
        }
        self.files.insert(path.to_owned(), value);
    }

    /// Whether a whole pass over the project has been read.
    pub fn complete(&self) -> bool {
        self.passes > 0
    }

    /// Forgets the files so the next scan starts over.
    pub fn reset(&mut self) {
        self.files.clear();
        self.next.clear();
        self.unread.clear();
        self.scanned = None;
        self.reading = false;
        self.passes = 0;
    }

    pub fn files(&self) -> &BTreeMap<String, T> {
        &self.files
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
        self.files.iter().map(|(p, v)| (p.as_str(), v))
    }
}

/// One [`Index`] per project root.
pub struct Indexes<T> {
    by_root: BTreeMap<String, Index<T>>,
    globs: &'static [&'static str],
    parse: fn(&str, &str) -> Option<T>,
}

impl<T: Clone> Indexes<T> {
    pub fn new(globs: &'static [&'static str], parse: fn(&str, &str) -> Option<T>) -> Self {
        Self { by_root: BTreeMap::new(), globs, parse }
    }

    /// The index of `project`, read for at most `budget` more.
    pub fn get(&mut self, project: &Project, budget: Duration) -> &mut Index<T> {
        let (globs, parse) = (self.globs, self.parse);
        let index = self.by_root.entry(project.root().unwrap_or_default()).or_insert_with(|| Index::new(globs, parse));
        index.scan(project, budget);
        index
    }

    /// The index of the project holding `path`, with the open document's text applied.
    pub fn for_document(&mut self, path: &str, text: &str, budget: Duration) -> Option<&mut Index<T>> {
        let project = project_of(path)?;
        let index = self.get(&project, budget);
        if index.globs.iter().any(|g| glob_matches(g, path)) {
            index.update(path, text);
        }
        Some(index)
    }

    /// Forgets every project's files.
    pub fn reset(&mut self) {
        self.by_root.clear();
    }
}

/// Whether `path` (absolute or relative) matches a project glob such as
/// `**/*.java` or `**/src/main/resources/**/*.yml`. `**` spans directories,
/// `*` stays within one; the glob is matched against the path's tail.
pub fn glob_matches(glob: &str, path: &str) -> bool {
    fn go(g: &[u8], p: &[u8]) -> bool {
        match g {
            [] => p.is_empty(),
            [b'*', b'*', b'/', rest @ ..] => (0..=p.len()).any(|i| (i == 0 || p[i - 1] == b'/') && go(rest, &p[i..])),
            [b'*', b'*', rest @ ..] => (0..=p.len()).any(|i| go(rest, &p[i..])),
            [b'*', rest @ ..] => (0..=p.len()).take_while(|&i| i == 0 || p[i - 1] != b'/').any(|i| go(rest, &p[i..])),
            [c, rest @ ..] => p.first() == Some(c) && go(rest, &p[1..]),
        }
    }
    let path = path.replace('\\', "/");
    let path = path.trim_start_matches('/');
    let glob = glob.trim_start_matches('/');
    // A relative glob may match from any directory of an absolute path.
    (0..=path.len()).any(|i| (i == 0 || path.as_bytes()[i - 1] == b'/') && go(glob.as_bytes(), &path.as_bytes()[i..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    fn words(_: &str, text: &str) -> Option<usize> {
        let n = text.split_whitespace().count();
        (n > 0).then_some(n)
    }

    #[test]
    fn reads_the_project_and_applies_open_documents() {
        let host = MockHost::install();
        host.add_file("src/A.java", "a b c");
        host.add_file("src/B.java", "");
        host.add_file("README.md", "x");
        let mut indexes = Indexes::new(&["**/*.java"], words);
        let project = host.invocation().project().unwrap();
        let index = indexes.get(&project, COMMAND_BUDGET);
        assert!(index.complete());
        assert_eq!(index.iter().map(|(p, n)| (p.rsplit('/').next().unwrap(), *n)).collect::<Vec<_>>(), [("A.java", 3)]);

        let index = indexes.for_document("/project/src/B.java", "one two", PROVIDER_BUDGET).unwrap();
        assert_eq!(index.files().keys().collect::<Vec<_>>(), ["/project/src/A.java", "/project/src/B.java"]);
        let index = indexes.for_document("/project/README.md", "x y", PROVIDER_BUDGET).unwrap();
        assert_eq!(index.files().len(), 2, "not one of the globs");
    }

    #[test]
    fn matches_globs() {
        assert!(glob_matches("**/*.java", "/p/src/A.java"));
        assert!(glob_matches("**/*.java", "A.java"));
        assert!(!glob_matches("**/*.java", "/p/A.javax"));
        assert!(glob_matches("**/application*.yml", "/p/src/main/resources/application-dev.yml"));
        assert!(!glob_matches("src/*.java", "/p/src/a/A.java"));
        assert!(glob_matches("**/resources/**/*.graphqls", "/p/src/main/resources/graphql/schema.graphqls"));
        assert!(glob_matches("**/resources/**/*.graphqls", "/p/src/main/resources/schema.graphqls"));
    }
}
