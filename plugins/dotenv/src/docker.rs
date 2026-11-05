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

/// `environment:` as a list (`- KEY=value`) or a mapping (`KEY: value`).
// ponytail: line-based, no flow style (`environment: [A=1]`) or anchors; a YAML parser if those matter.
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
                if let Some((key, value)) = item.filter(|(key, _)| !key.is_empty()) {
                    let start = item_at + content[item_at - at..].find(key).unwrap_or(0);
                    out.push((key.to_owned(), value.to_owned(), start..start + key.len()));
                }
                continue;
            }
            environment = None;
        }

        let after = content.strip_prefix("environment:").map(str::trim);
        if after.is_some_and(|rest| rest.is_empty() || rest.starts_with('#')) {
            environment = Some(indent);
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
}
