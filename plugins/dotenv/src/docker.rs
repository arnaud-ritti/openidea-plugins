//! Variables declared for containers: `ENV` in Dockerfiles and `environment:`
//! in docker-compose files, after the original plugin's Docker module
//! (`DockerfilePsiElementsVisitor`, `DockerComposeYamlPsiElementsVisitor`).

use std::ops::Range;

/// A declared key, its value and the key's byte range.
pub type Entry = (String, String, Range<usize>);

fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if let Some(inner) = s.strip_prefix(q).and_then(|s| s.strip_suffix(q)) {
            return inner;
        }
    }
    s
}

/// `(start, word)` for each blank-separated word of `s`, quotes kept together.
fn words(s: &str) -> Vec<(usize, &str)> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let mut quote = None;
        while i < bytes.len() && (quote.is_some() || !bytes[i].is_ascii_whitespace()) {
            match (bytes[i], quote) {
                (b'\\', _) => i += 1,
                (b'"' | b'\'', None) => quote = Some(bytes[i]),
                (q, Some(open)) if q == open => quote = None,
                _ => {}
            }
            i += 1;
        }
        let end = i.min(bytes.len());
        out.push((start, &s[start..end]));
    }
    out
}

/// `ENV KEY=value OTHER="x y"`, `ENV KEY value`, continued with `\`.
pub fn dockerfile(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut continued = false;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let body = line.trim_end_matches(['\n', '\r']);
        let content = body.trim_start();
        let mut at = line_start + body.len() - content.len();
        let rest = if continued {
            content
        } else if content.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("env ")) {
            at += 4;
            &content[4..]
        } else {
            continue;
        };
        let rest = rest.trim_end();
        continued = rest.ends_with('\\');
        let rest = rest.trim_end_matches('\\');

        let words = words(rest);
        let Some(&(_, first)) = words.first() else { continue };
        if first.contains('=') {
            for (start, word) in words {
                let Some((key, value)) = word.split_once('=') else { continue };
                if !key.is_empty() {
                    out.push((key.to_owned(), unquote(value).to_owned(), at + start..at + start + key.len()));
                }
            }
        } else {
            let start = words[0].0;
            let value = rest[start + first.len()..].trim();
            out.push((first.to_owned(), unquote(value).to_owned(), at + start..at + start + first.len()));
        }
    }
    out
}

/// Merge keys, aliases, anchors and `${…}` interpolations aren't variable names.
fn is_anchor_syntax(key: &str) -> bool {
    let mut chars = key.chars();
    let starts_well = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    !starts_well || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_comment_or_empty(rest: &str) -> bool {
    rest.is_empty() || rest.starts_with('#')
}

/// Items of a one-line flow collection `[a, b]` / `{a: 1}`: `(start, item)` with
/// start relative to `s`, split on commas outside quotes.
fn flow_items(s: &str) -> Option<Vec<(usize, &str)>> {
    let s = s.trim_end();
    let close = match s.as_bytes().first()? {
        b'[' => b']',
        b'{' => b'}',
        _ => return None,
    };
    let end = s.rfind(char::from(close))?;
    let inner = &s[1..end];
    let mut items = Vec::new();
    let mut start = 0;
    let mut quote = None;
    for (i, c) in inner.char_indices() {
        match (c, quote) {
            ('"' | '\'', None) => quote = Some(c),
            (q, Some(open)) if q == open => quote = None,
            (',', None) => {
                items.push((start + 1, &inner[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push((start + 1, &inner[start..]));
    Some(items)
}

fn entry_of(item: &str, item_at: usize, map: bool) -> Option<Entry> {
    let lead = item.len() - item.trim_start().len();
    let item = item.trim();
    let (key, value) = if map {
        item.split_once(':').map(|(k, v)| (unquote(k), unquote(v)))?
    } else {
        let inner = unquote(item);
        let (k, v) = inner.split_once('=').unwrap_or((inner, ""));
        (k.trim(), v.trim())
    };
    if is_anchor_syntax(key) {
        return None;
    }
    let start = item_at + lead + item.find(key)?;
    let value = if value == "~" || value == "null" { "" } else { value };
    Some((key.to_owned(), value.to_owned(), start..start + key.len()))
}

/// `environment:` as a list (`- KEY=value`), a mapping (`KEY: value`) or a
/// one-line flow collection (`[A=1, B]`, `{A: 1}`). Anchors, aliases and merge
/// keys are skipped: `environment: *env` and `<<: *common` declare nothing.
// ponytail: line-based; a multi-line flow collection or an anchored block (`x-env: &env`) isn't read; a YAML parser if that matters.
pub fn compose(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut environment: Option<usize> = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let body = line.trim_end_matches(['\n', '\r']);
        let content = body.trim_start();
        let indent = body.len() - content.len();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }

        if let Some(parent) = environment {
            if indent > parent {
                let at = line_start + indent;
                let (item, item_at) = match content.strip_prefix('-') {
                    Some(item) => {
                        let item = item.trim_start();
                        let inner = unquote(item);
                        let (key, value) = inner.split_once('=').unwrap_or((inner, ""));
                        (Some((key.trim(), value.trim())), at + content.len() - item.len())
                    }
                    None => (content.split_once(':').map(|(k, v)| (unquote(k), unquote(v))), at),
                };
                if let Some((key, value)) = item.filter(|(key, _)| !is_anchor_syntax(key)) {
                    let start = item_at + content[item_at - at..].find(key).unwrap_or(0);
                    out.push((key.to_owned(), value.to_owned(), start..start + key.len()));
                }
                continue;
            }
            environment = None;
        }

        let Some(rest) = content.strip_prefix("environment:") else { continue };
        let rest = rest.trim();
        if is_comment_or_empty(rest) || (rest.starts_with('&') && !rest.contains(char::is_whitespace)) {
            environment = Some(indent);
            continue;
        }
        let rest_at = line_start + indent + "environment:".len() + (content["environment:".len()..].len() - content["environment:".len()..].trim_start().len());
        if let Some(items) = flow_items(rest) {
            let map = rest.starts_with('{');
            out.extend(items.into_iter().filter_map(|(at, item)| entry_of(item, rest_at + at, map)));
        }
    }
    out
}

/// Paths named by `env_file:` (scalar, list, or `- path: x` long form), as
/// written in the compose file.
pub fn compose_env_files(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut parent: Option<usize> = None;
    for line in text.lines() {
        let content = line.trim_start();
        let indent = line.len() - content.len();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        if let Some(p) = parent {
            if indent > p {
                let item = content.strip_prefix('-').unwrap_or(content).trim_start();
                let item = item.strip_prefix("path:").unwrap_or(item);
                let item = unquote(item.split(" #").next().unwrap_or(item));
                if !item.is_empty() && !item.starts_with(['*', '&']) && !item.contains(": ") && !item.ends_with(':') {
                    out.push(item.to_owned());
                }
                continue;
            }
            parent = None;
        }
        let Some(rest) = content.strip_prefix("env_file:") else { continue };
        let rest = rest.trim();
        if is_comment_or_empty(rest) {
            parent = Some(indent);
        } else if let Some(items) = flow_items(rest) {
            out.extend(items.into_iter().map(|(_, i)| unquote(i).to_owned()).filter(|i| !i.is_empty()));
        } else {
            let value = unquote(rest.split(" #").next().unwrap_or(rest));
            if !value.starts_with(['*', '&']) {
                out.push(value.to_owned());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(text: &str, entries: Vec<Entry>) -> Vec<(String, String, String)> {
        entries.into_iter().map(|(k, v, r)| (k, v, text[r].to_owned())).collect()
    }

    fn e(k: &str, v: &str) -> (String, String, String) {
        (k.into(), v.into(), k.into())
    }

    #[test]
    fn reads_dockerfile_env() {
        let text = "FROM php\nENV APP_ENV=prod NAME=\"a b\" \\\n    DEBUG=0\nenv   LEGACY some value\nRUN echo $X\n";
        assert_eq!(
            keys(text, dockerfile(text)),
            [e("APP_ENV", "prod"), e("NAME", "a b"), e("DEBUG", "0"), e("LEGACY", "some value")]
        );
    }

    #[test]
    fn reads_compose_environment() {
        let text = "services:\n  app:\n    environment:\n      - DB_HOST=db\n      - \"QUOTED=1\"\n      - BARE\n    ports:\n      - \"80:80\"\n  worker:\n    environment:\n      QUEUE: redis\n      'TIMEOUT': \"30\"\n";
        assert_eq!(
            keys(text, compose(text)),
            [e("DB_HOST", "db"), e("QUOTED", "1"), e("BARE", ""), e("QUEUE", "redis"), e("TIMEOUT", "30")]
        );
    }

    #[test]
    fn compose_env_file_list_and_scalar_are_declared() {
        let text = "services:\n  a:\n    env_file: .env\n  b:\n    env_file:\n      - ./x.env\n      - path: \"./y.env\"\n        required: false\n  c:\n    env_file: [z.env, \"w.env\"]\n";
        assert_eq!(compose_env_files(text), [".env", "./x.env", "./y.env", "z.env", "w.env"]);
    }

    #[test]
    fn flow_style_environment_array_is_read() {
        let text = "services:\n  a:\n    environment: [A=1, \"B\", 'C=x y' , ]\n  b:\n    environment: {A: 1, B: ~, \"C\": \"two words\"}\n";
        assert_eq!(
            keys(text, compose(text)),
            [e("A", "1"), e("B", ""), e("C", "x y"), e("A", "1"), e("B", ""), e("C", "two words")]
        );
    }

    #[test]
    fn anchor_merge_keys_are_skipped_not_crashing() {
        let text = "x-env: &env\n  X: 1\nservices:\n  a:\n    environment: *env\n  b:\n    environment:\n      <<: *env\n      Y: 2\n  c:\n    environment: &local\n      - *env\n      - Z=3\n";
        assert_eq!(keys(text, compose(text)), [e("Y", "2"), e("Z", "3")]);
    }

    #[test]
    fn interpolated_items_are_not_declared() {
        let text = "services:\n  a:\n    environment: [${A}=1, \"${VAR}\", OK=1]\n  b:\n    environment:\n      - ${B}=2\n      - C=3\n";
        assert_eq!(keys(text, compose(text)), [e("OK", "1"), e("C", "3")]);
    }
}
