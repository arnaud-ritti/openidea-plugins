//! gitignore patterns: does an entry match a path. Used to find entries that
//! another entry already covers and to tell whether an open file is ignored.

/// One gitignore entry, parsed.
#[derive(Debug, PartialEq)]
pub struct Rule {
    pub negated: bool,
    pub pattern: String,
    /// Contains a `/` (other than a trailing one): matched against the path
    /// from the ignore file's directory, else against every name in it.
    pub anchored: bool,
    /// Ends with `/`: matches directories only.
    pub dir_only: bool,
}

impl Rule {
    /// `None` for blank lines and comments.
    pub fn parse(entry: &str) -> Option<Self> {
        let entry = entry.trim();
        if entry.is_empty() || entry.starts_with('#') {
            return None;
        }

        let (negated, rest) = match entry.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, entry.strip_prefix('\\').filter(|r| r.starts_with(['!', '#'])).unwrap_or(entry)),
        };
        let dir_only = rest.ends_with('/');
        let rest = rest.trim_end_matches('/');
        let anchored = rest.contains('/');
        let pattern = rest.strip_prefix('/').unwrap_or(rest).to_owned();
        Some(Self { negated, pattern, anchored, dir_only })
    }

    /// Whether the rule matches `path` (relative to the ignore file's
    /// directory) or one of its parent directories.
    pub fn matches(&self, path: &str, is_dir: bool) -> bool {
        let ends = path.match_indices('/').map(|(i, _)| i).chain([path.len()]);
        ends.into_iter().any(|end| {
            let prefix = &path[..end];
            if self.dir_only && end == path.len() && !is_dir {
                return false;
            }
            let candidate = match self.anchored {
                true => prefix,
                false => prefix.rsplit('/').next().unwrap_or(prefix),
            };
            wildmatch(self.pattern.as_bytes(), candidate.as_bytes())
        })
    }

    /// No wildcard: the entry names one path.
    pub fn is_literal(&self) -> bool {
        let mut bytes = self.pattern.bytes();
        while let Some(b) = bytes.next() {
            match b {
                b'\\' => {
                    bytes.next();
                }
                b'*' | b'?' | b'[' => return false,
                _ => {}
            }
        }
        true
    }

    pub fn literal_path(&self) -> String {
        crate::check::unescape(&self.pattern)
    }
}

/// gitignore's wildmatch: `*`, `?` and `[...]` stay within one name, `**`
/// spans directories.
// ponytail: backtracking recursion, exponential on many `*`s; entries are short.
fn wildmatch(p: &[u8], t: &[u8]) -> bool {
    match p {
        [] => t.is_empty(),
        [b'*', b'*', b'/', rest @ ..] => {
            wildmatch(rest, t) || t.iter().enumerate().any(|(i, &c)| c == b'/' && wildmatch(rest, &t[i + 1..]))
        }
        [b'*', b'*', rest @ ..] => (0..=t.len()).any(|i| wildmatch(rest, &t[i..])),
        [b'*', rest @ ..] => (0..=t.len()).take_while(|&i| !t[..i].contains(&b'/')).any(|i| wildmatch(rest, &t[i..])),
        [b'?', rest @ ..] => matches!(t, [c, ..] if *c != b'/') && wildmatch(rest, &t[1..]),
        [b'[', class @ ..] => match (t, class_match(class, t.first().copied())) {
            ([_, tail @ ..], Some((true, len))) => wildmatch(&class[len..], tail),
            (_, Some((false, _))) => false,
            _ => t.first() == Some(&b'[') && wildmatch(class, &t[1..]),
        },
        [b'\\', c, rest @ ..] => t.first() == Some(c) && wildmatch(rest, &t[1..]),
        [c, rest @ ..] => t.first() == Some(c) && wildmatch(rest, &t[1..]),
    }
}

/// Matches `c` against the class after its `[`: whether it matched and the
/// length of the class up to and including `]`. `None`: no closing `]`.
fn class_match(class: &[u8], c: Option<u8>) -> Option<(bool, usize)> {
    let negated = matches!(class.first(), Some(b'!' | b'^'));
    let mut i = usize::from(negated);
    let mut matched = false;
    let mut first = true;

    loop {
        let b = *class.get(i)?;
        if b == b']' && !first {
            let hit = c.is_some_and(|c| c != b'/') && matched != negated;
            return Some((hit, i + 1));
        }
        first = false;

        if b == b'[' && class.get(i + 1) == Some(&b':') {
            let end = class[i + 2..].windows(2).position(|w| w == b":]")? + i + 2;
            matched |= c.is_some_and(|c| posix_class(&class[i + 2..end], c));
            i = end + 2;
            continue;
        }

        let low = if b == b'\\' {
            i += 1;
            *class.get(i)?
        } else {
            b
        };
        if class.get(i + 1) == Some(&b'-') && class.get(i + 2).is_some_and(|&h| h != b']') {
            let high = class[i + 2];
            matched |= c.is_some_and(|c| (low..=high).contains(&c));
            i += 3;
        } else {
            matched |= c == Some(low);
            i += 1;
        }
    }
}

fn posix_class(name: &[u8], c: u8) -> bool {
    match name {
        b"alnum" => c.is_ascii_alphanumeric(),
        b"alpha" => c.is_ascii_alphabetic(),
        b"blank" => c == b' ' || c == b'\t',
        b"digit" => c.is_ascii_digit(),
        b"lower" => c.is_ascii_lowercase(),
        b"punct" => c.is_ascii_punctuation(),
        b"space" => c.is_ascii_whitespace(),
        b"upper" => c.is_ascii_uppercase(),
        b"xdigit" => c.is_ascii_hexdigit(),
        _ => false,
    }
}

/// The ignore files that apply to a path, outermost first: (directory relative
/// to the project root, "" for the root; display name; content).
pub type Sources = Vec<(String, String, String)>;

/// The entry that ignores `path` (relative to the project root) and the name
/// of its file; `None` when the last matching entry is a negation or none
/// matches.
// ponytail: a negation re-includes files under an ignored directory, which git doesn't; fine for a hint.
pub fn ignored_by<'a>(sources: &'a Sources, path: &str) -> Option<(&'a str, &'a str)> {
    let mut verdict = None;
    for (dir, name, text) in sources {
        let relative = match dir.as_str() {
            "" => path,
            dir => match path.strip_prefix(dir).and_then(|p| p.strip_prefix('/')) {
                Some(relative) => relative,
                None => continue,
            },
        };
        for line in text.lines() {
            let Some(rule) = Rule::parse(line) else { continue };
            if rule.matches(relative, false) {
                verdict = (!rule.negated).then_some((name.as_str(), line.trim()));
            }
        }
    }
    verdict
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(entry: &str, path: &str) -> bool {
        Rule::parse(entry).unwrap().matches(path, false)
    }

    #[test]
    fn wildcards_stay_within_a_name() {
        assert!(matches("*.log", "logs/debug.log"));
        assert!(matches("debug?.log", "debug1.log"));
        assert!(!matches("/*.log", "logs/debug.log"));
        assert!(matches("/*.log", "debug.log"));
        assert!(!matches("a/*.c", "a/b/x.c"));
    }

    #[test]
    fn double_star_spans_directories() {
        assert!(matches("**/build", "a/b/build"));
        assert!(matches("**/build", "build"));
        assert!(matches("a/**/z", "a/z"));
        assert!(matches("a/**/z", "a/b/c/z"));
        assert!(matches("logs/**", "logs/a/b.txt"));
    }

    #[test]
    fn directories_match_their_contents() {
        assert!(matches("build/", "build/out/app.o"));
        assert!(!matches("build/", "build"));
        assert!(Rule::parse("build/").unwrap().matches("build", true));
        assert!(matches("/target", "target/debug/app"));
        assert!(!matches("/target", "src/target"));
        assert!(matches("target", "src/target"));
    }

    #[test]
    fn classes_and_escapes() {
        assert!(matches("[Bb]in", "Bin"));
        assert!(matches("*.[oa]", "lib.a"));
        assert!(!matches("*.[!oa]", "lib.a"));
        assert!(matches("file[0-9]", "file7"));
        assert!(matches("[[:digit:]]x", "1x"));
        assert!(matches("[]]", "]"));
        assert!(matches("\\#notes", "#notes"));
        assert!(matches("out\\[1\\].log", "out[1].log"));
        assert!(matches("[abc", "[abc"));
    }

    #[test]
    fn last_matching_entry_wins_across_files() {
        let sources: Sources = vec![
            (String::new(), ".gitignore".into(), "*.log\n!keep.log\n".into()),
            ("app".into(), "app/.gitignore".into(), "cache/\n".into()),
        ];
        assert_eq!(ignored_by(&sources, "debug.log"), Some((".gitignore", "*.log")));
        assert_eq!(ignored_by(&sources, "keep.log"), None);
        assert_eq!(ignored_by(&sources, "app/cache/x.bin"), Some(("app/.gitignore", "cache/")));
        assert_eq!(ignored_by(&sources, "cache/x.bin"), None);
        assert_eq!(ignored_by(&sources, "src/main.rs"), None);
    }
}
