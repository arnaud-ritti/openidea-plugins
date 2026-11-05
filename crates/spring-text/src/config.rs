//! Keys of Spring configuration files: `.properties` lines and YAML block
//! mappings and sequences, as full dotted paths (`spring.datasource.url`,
//! `spring.cloud.gateway.routes[0].predicates[1]`). Offsets are UTF-8 bytes.

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Key {
    /// The full path.
    pub key: String,
    /// The bytes of the key as written (its last segment in YAML; the value of
    /// a sequence item that has no key).
    pub at: usize,
    pub end: usize,
    /// The value, unquoted, without a trailing comment (empty for a YAML parent key).
    pub value: String,
    pub value_at: usize,
    pub value_end: usize,
}

/// Whether `path` names a Spring configuration file (`application.yml`,
/// `application-dev.properties`, `bootstrap.yaml`…).
pub fn is_config_file(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let Some((stem, ext)) = name.rsplit_once('.') else { return false };
    matches!(ext, "properties" | "yml" | "yaml")
        && ["application", "bootstrap"].iter().any(|p| stem == *p || stem.starts_with(&format!("{p}-")))
}

/// The keys of a `.properties` text.
pub fn properties(text: &str) -> Vec<Key> {
    let mut out = Vec::new();
    let mut offset = 0;
    let mut continued = false;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let body = line.trim_end_matches(['\n', '\r']);
        let was_continued = continued;
        continued = body.chars().rev().take_while(|&c| c == '\\').count() % 2 == 1;
        if was_continued {
            continue;
        }
        let trimmed = body.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
            continue;
        }
        let at = start + body.len() - trimmed.len();
        let bytes = trimmed.as_bytes();
        let mut k = 0;
        while k < bytes.len() && !matches!(bytes[k], b'=' | b':' | b' ' | b'\t') {
            k += if bytes[k] == b'\\' { 2 } else { 1 };
        }
        let k = k.min(bytes.len());
        let mut v = k;
        while v < bytes.len() && matches!(bytes[v], b' ' | b'\t') {
            v += 1;
        }
        if v < bytes.len() && matches!(bytes[v], b'=' | b':') {
            v += 1;
            while v < bytes.len() && matches!(bytes[v], b' ' | b'\t') {
                v += 1;
            }
        }
        let value = trimmed[v..].trim_end();
        out.push(Key {
            key: trimmed[..k].replace('\\', ""),
            at,
            end: at + k,
            value: value.to_owned(),
            value_at: at + v,
            value_end: at + v + value.len(),
        });
    }
    out
}

struct Level {
    indent: usize,
    /// `name`, or `[n]` for a sequence item.
    segment: String,
    items: usize,
}

fn join(levels: &[Level], last: &str) -> String {
    let mut path = String::new();
    for seg in levels.iter().map(|l| l.segment.as_str()).chain(std::iter::once(last)) {
        if seg.is_empty() {
            continue;
        }
        if !path.is_empty() && !seg.starts_with('[') {
            path.push('.');
        }
        path.push_str(seg);
    }
    path
}

/// Splits `content` into a mapping key and the bytes where its value starts.
fn split_key(content: &str) -> Option<(&str, usize, usize, usize)> {
    let (key_from, key_to, after) = match content.as_bytes().first()? {
        q @ (b'"' | b'\'') => {
            let close = content[1..].find(*q as char)? + 1;
            (1, close, close + 1)
        }
        b'#' | b'{' | b'[' | b'&' | b'*' | b'!' | b'|' | b'>' => return None,
        _ => {
            let colon = content.match_indices(':').find(|(i, _)| {
                content[i + 1..].is_empty() || content[i + 1..].starts_with([' ', '\t'])
            })?;
            (0, content[..colon.0].trim_end().len(), colon.0)
        }
    };
    if !content[after..].starts_with(':') {
        return None;
    }
    Some((&content[key_from..key_to], key_from, key_to, after + 1))
}

/// Strips a trailing ` # comment` and surrounding quotes: the value and its bytes in `s`.
fn scalar(s: &str) -> (String, usize, usize) {
    let lead = s.len() - s.trim_start().len();
    let mut body = &s[lead..];
    if let Some(q @ ('"' | '\'')) = body.chars().next()
        && let Some(close) = body[1..].find(q)
    {
        return (body[1..close + 1].to_owned(), lead + 1, lead + 1 + close);
    }
    if let Some(hash) = body.find(" #") {
        body = &body[..hash];
    }
    let body = body.trim_end();
    (body.to_owned(), lead, lead + body.len())
}

/// Walks YAML lines, keeping the open mappings and sequences.
#[derive(Default)]
struct Walker {
    levels: Vec<Level>,
    /// Lines more indented than this belong to a block scalar.
    block: Option<usize>,
}

impl Walker {
    /// Feeds one line (without its newline) starting at byte `start`; pushes its keys to `out`.
    fn line(&mut self, line: &str, start: usize, out: &mut Vec<Key>) {
        let content = line.trim_start_matches(' ');
        let mut indent = line.len() - content.len();
        let content = content.trim_end();
        if let Some(block) = self.block {
            if content.is_empty() || indent > block {
                return;
            }
            self.block = None;
        }
        if content.is_empty() || content.starts_with('#') {
            return;
        }
        if indent == 0 && (content == "---" || content == "..." || content.starts_with("--- ")) {
            self.levels.clear();
            return;
        }
        let mut content = content;
        while content == "-" || content.starts_with("- ") {
            // A sequence item: its parent is the mapping key above (at a
            // lower indent, or the same one) or the enclosing item.
            while self.levels.last().is_some_and(|l| l.indent > indent || l.indent == indent && l.segment.starts_with('['))
            {
                self.levels.pop();
            }
            let n = self.levels.last_mut().map_or(0, |l| {
                l.items += 1;
                l.items - 1
            });
            self.levels.push(Level { indent, segment: format!("[{n}]"), items: 0 });
            let rest = &content[1..];
            let skip = rest.len() - rest.trim_start().len() + 1;
            indent += skip;
            content = rest.trim_start();
            if content.is_empty() {
                return;
            }
        }
        let base = start + indent;
        if let Some((key, from, to, after)) = split_key(content) {
            while self.levels.last().is_some_and(|l| l.indent >= indent) {
                self.levels.pop();
            }
            let (value, v_from, v_to) = scalar(&content[after..]);
            let path = join(&self.levels, key);
            out.push(Key {
                key: path,
                at: base + from,
                end: base + to,
                value: value.clone(),
                value_at: base + after + v_from,
                value_end: base + after + v_to,
            });
            if value.is_empty() {
                self.levels.push(Level { indent, segment: key.to_owned(), items: 0 });
            } else if value.starts_with(['|', '>']) {
                self.block = Some(indent);
            }
            return;
        }
        // A scalar sequence item.
        if self.levels.last().is_some_and(|l| l.segment.starts_with('[')) {
            let (value, v_from, v_to) = scalar(content);
            let key = join(&self.levels, "");
            out.push(Key {
                key,
                at: base + v_from,
                end: base + v_to,
                value,
                value_at: base + v_from,
                value_end: base + v_to,
            });
        }
    }
}

fn lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut offset = 0;
    text.split_inclusive('\n').map(move |l| {
        let start = offset;
        offset += l.len();
        (start, l.trim_end_matches(['\n', '\r']))
    })
}

/// The keys of a YAML text (block style; flow collections are skipped).
pub fn yaml(text: &str) -> Vec<Key> {
    let mut walker = Walker::default();
    let mut out = Vec::new();
    for (start, line) in lines(text) {
        walker.line(line, start, &mut out);
    }
    out
}

/// The path a key typed at byte `at` of a YAML text would be under
/// (`spring.datasource` while typing under `datasource:`), and what is typed
/// so far on the line before `at`.
pub fn yaml_parent(text: &str, at: usize) -> (String, String) {
    let at = at.min(text.len());
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let mut walker = Walker::default();
    let mut sink = Vec::new();
    for (start, line) in lines(&text[..line_start]) {
        walker.line(line, start, &mut sink);
    }
    let typed = &text[line_start..at];
    let content = typed.trim_start_matches(' ');
    let indent = typed.len() - content.len();
    let mut content = content;
    let mut levels = walker.levels;
    if content.starts_with("- ") {
        while levels.last().is_some_and(|l| l.indent > indent || l.indent == indent && l.segment.starts_with('[')) {
            levels.pop();
        }
        let n = levels.last().map_or(0, |l| l.items);
        levels.push(Level { indent, segment: format!("[{n}]"), items: 0 });
        content = content[2..].trim_start();
    } else {
        while levels.last().is_some_and(|l| l.indent >= indent) {
            levels.pop();
        }
    }
    (join(&levels, ""), content.to_owned())
}

/// The keys of a configuration file, by its extension.
pub fn keys(path: &str, text: &str) -> Vec<Key> {
    if path.ends_with(".properties") { properties(text) } else { yaml(text) }
}

/// Spring's relaxed form of a key for comparison: `spring.jpa.showSql`,
/// `spring.jpa.show_sql` and `SPRING_JPA_SHOW-SQL` all become `spring.jpa.showsql`.
pub fn canonical(key: &str) -> String {
    key.chars().filter(|c| !matches!(c, '-' | '_')).flat_map(|c| c.to_lowercase()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_properties() {
        let text = "# comment\nserver.port=8080\nspring.application.name : demo # not a comment\nlong=a\\\n  b\n  key\\ with\\ space = v\n";
        let keys: Vec<_> = properties(text).into_iter().map(|k| (k.key, k.value)).collect();
        assert_eq!(keys, [
            ("server.port".to_owned(), "8080".to_owned()),
            ("spring.application.name".into(), "demo # not a comment".into()),
            ("long".into(), "a\\".into()),
            ("key with space".into(), "v".into()),
        ]);
        let k = &properties(text)[0];
        assert_eq!(&text[k.at..k.end], "server.port");
        assert_eq!(&text[k.value_at..k.value_end], "8080");
    }

    const YAML: &str = "spring:
  datasource:
    url: jdbc:h2:mem:test   # inline comment
    'user-name': sa
  cloud:
    gateway:
      routes:
        - id: users
          uri: lb://users
          predicates:
            - Path=/users/**
            - Method=GET
          filters:
          - StripPrefix=1
        - id: orders
  sql:
    init:
      schema: |
        create table x;
        drop: no
server:
  port: \"8080\"
---
logging.level.root: INFO
";

    #[test]
    fn reads_yaml() {
        let keys: Vec<_> = yaml(YAML).into_iter().map(|k| (k.key, k.value)).collect();
        let keys: Vec<_> = keys.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(keys, [
            ("spring", ""),
            ("spring.datasource", ""),
            ("spring.datasource.url", "jdbc:h2:mem:test"),
            ("spring.datasource.user-name", "sa"),
            ("spring.cloud", ""),
            ("spring.cloud.gateway", ""),
            ("spring.cloud.gateway.routes", ""),
            ("spring.cloud.gateway.routes[0].id", "users"),
            ("spring.cloud.gateway.routes[0].uri", "lb://users"),
            ("spring.cloud.gateway.routes[0].predicates", ""),
            ("spring.cloud.gateway.routes[0].predicates[0]", "Path=/users/**"),
            ("spring.cloud.gateway.routes[0].predicates[1]", "Method=GET"),
            ("spring.cloud.gateway.routes[0].filters", ""),
            ("spring.cloud.gateway.routes[0].filters[0]", "StripPrefix=1"),
            ("spring.cloud.gateway.routes[1].id", "orders"),
            ("spring.sql", ""),
            ("spring.sql.init", ""),
            ("spring.sql.init.schema", "|"),
            ("server", ""),
            ("server.port", "8080"),
            ("logging.level.root", "INFO"),
        ]);
        let url = yaml(YAML).into_iter().find(|k| k.key == "spring.datasource.url").unwrap();
        assert_eq!(&YAML[url.at..url.end], "url");
        assert_eq!(&YAML[url.value_at..url.value_end], "jdbc:h2:mem:test");
        let user = yaml(YAML).into_iter().find(|k| k.key.ends_with("user-name")).unwrap();
        assert_eq!(&YAML[user.at..user.end], "user-name");
        let path = yaml(YAML).into_iter().find(|k| k.value == "Path=/users/**").unwrap();
        assert_eq!(&YAML[path.at..path.end], "Path=/users/**");
        let port = yaml(YAML).into_iter().find(|k| k.key == "server.port").unwrap();
        assert_eq!(&YAML[port.value_at..port.value_end], "8080");
    }

    #[test]
    fn finds_the_parent_of_a_key_being_typed() {
        let text = "spring:\n  datasource:\n    url: x\n    pass";
        assert_eq!(yaml_parent(text, text.len()), ("spring.datasource".into(), "pass".into()));
        let text = "spring:\n  datasource:\n    url: x\n  jp";
        assert_eq!(yaml_parent(text, text.len()), ("spring".into(), "jp".into()));
        let text = "routes:\n  - id: a\n    predicates:\n      - Pa";
        assert_eq!(yaml_parent(text, text.len()), ("routes[0].predicates[0]".into(), "Pa".into()));
        assert_eq!(yaml_parent("ser", 3), (String::new(), "ser".into()));
    }

    #[test]
    fn survives_every_prefix() {
        for end in 0..=YAML.len() {
            yaml(&YAML[..end]);
            yaml_parent(YAML, end);
            properties(&YAML[..end]);
        }
    }

    #[test]
    fn names_config_files() {
        assert!(is_config_file("/p/src/main/resources/application.yml"));
        assert!(is_config_file("application-dev.properties"));
        assert!(is_config_file("bootstrap.yaml"));
        assert!(!is_config_file("messages.properties"));
        assert!(!is_config_file("applications.yml"));
        assert_eq!(canonical("spring.jpa.show-sql"), canonical("spring.jpa.showSql"));
    }
}
