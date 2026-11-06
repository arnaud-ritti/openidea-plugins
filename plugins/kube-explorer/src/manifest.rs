//! A block scanner for Kubernetes manifests, not a YAML parser: it splits a
//! text into documents and reads the few top-level facts the checks need
//! (`apiVersion`, `kind`, `metadata.name`, `spec.selector`, the `name:` keys of
//! `env:` lists) with byte offsets. Flow style (`{...}`, `[...]`) is left
//! alone and makes the matching check stay quiet.

use std::ops::Range;

/// A scalar value without its quotes, and where it sits in the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Value {
    pub text: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Doc {
    pub api_version: Option<Value>,
    pub kind: Option<Value>,
    pub name: Option<Value>,
    pub has_generate_name: bool,
    pub has_selector: bool,
    /// `metadata` is written in flow style, so no name can be judged.
    pub name_unknown: bool,
    /// The `name:` values of each block-style `env:` list.
    pub env_lists: Vec<Vec<Value>>,
    pub top_keys: Vec<String>,
    /// Bytes of the document, after its `---` line.
    pub range: Range<usize>,
    pub start_line: usize,
}

impl Doc {
    pub fn is_manifest(&self) -> bool {
        self.api_version.is_some() && self.kind.is_some()
    }
}

struct Line<'a> {
    start: usize,
    text: &'a str,
}

impl Line<'_> {
    fn indent(&self) -> usize {
        self.text.len() - self.text.trim_start_matches(' ').len()
    }

    fn content(&self) -> &str {
        self.text.trim_start()
    }

    fn skip(&self) -> bool {
        let content = self.content();
        content.is_empty() || content.starts_with('#')
    }
}

fn lines(text: &str) -> Vec<Line<'_>> {
    let mut start = 0;
    text.split_inclusive('\n')
        .map(|raw| {
            let line = Line { start, text: raw.trim_end_matches(['\n', '\r']) };
            start += raw.len();
            line
        })
        .collect()
}

fn is_separator(line: &Line) -> bool {
    line.text.strip_prefix("---").is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
}

/// The documents of `text`; a document that is empty between two separators is left out.
pub fn documents(text: &str) -> Vec<Doc> {
    let all = lines(text);
    let mut docs = Vec::new();
    let mut from = 0;
    for to in (0..=all.len()).filter(|&i| i == all.len() || is_separator(&all[i])) {
        if from < to {
            let end = all.get(to).map_or(text.len(), |line| line.start);
            let mut doc = scan(&all[from..to]);
            doc.range = all[from].start..end;
            doc.start_line = from;
            docs.push(doc);
        }
        from = to + 1;
    }
    docs
}

/// The key of a `key: value` line and its value; `None` for list items, comments and flow lines.
fn key_value(content: &str, at: usize) -> Option<(String, Option<Value>)> {
    if content.starts_with(['-', '#', '[', '{', '?']) {
        return None;
    }
    let (key, after) = match content.chars().next()? {
        quote @ ('"' | '\'') => {
            let close = content[1..].find(quote)? + 1;
            (&content[1..close], close + 1)
        }
        _ => {
            let colon = content.char_indices().find_map(|(i, c)| {
                let ends = c == ':' && (content[i + 1..].is_empty() || content[i + 1..].starts_with([' ', '\t']));
                ends.then_some(i)
            })?;
            (&content[..colon], colon)
        }
    };
    let rest = content[after..].strip_prefix(':')?;
    let base = at + after + 1;
    Some((key.trim().to_owned(), value(rest, base)))
}

fn value(rest: &str, base: usize) -> Option<Value> {
    let lead = rest.len() - rest.trim_start().len();
    let v = rest.trim_start();
    let (text, offset) = match v.chars().next()? {
        '#' => return None,
        quote @ ('"' | '\'') => match v[1..].find(quote) {
            Some(close) => (&v[1..close + 1], 1),
            None => (v, 0),
        },
        _ => (v.find(" #").map_or(v, |i| &v[..i]).trim_end(), 0),
    };
    let start = base + lead + offset;
    Some(Value { text: text.to_owned(), start, end: start + text.len() })
}

/// Lines inside a `|` or `>` block scalar, whose text is data, not structure.
fn scalar_mask(lines: &[Line]) -> Vec<bool> {
    let mut mask = vec![false; lines.len()];
    let mut i = 0;
    while i < lines.len() {
        i += 1;
        let line = &lines[i - 1];
        if line.skip() {
            continue;
        }
        let mut content = line.content();
        while let Some(rest) = content.strip_prefix("- ") {
            content = rest.trim_start();
        }
        let column = line.indent() + line.content().len() - content.len();
        let header = match key_value(content, 0) {
            Some((_, Some(v))) => v.text,
            Some((_, None)) => continue,
            None => content.to_owned(),
        };
        if !header.starts_with(['|', '>']) {
            continue;
        }
        while i < lines.len() && (lines[i].text.trim().is_empty() || lines[i].indent() > column) {
            mask[i] = true;
            i += 1;
        }
    }
    mask
}

/// The lines directly under line `parent`, at the indent of the first one.
fn children(lines: &[Line], mask: &[bool], parent: usize) -> Vec<usize> {
    let parent_indent = lines[parent].indent();
    let mut found = Vec::new();
    let mut indent = None;
    for (j, line) in lines.iter().enumerate().skip(parent + 1) {
        if mask[j] || line.skip() {
            continue;
        }
        if line.indent() <= parent_indent {
            break;
        }
        if line.indent() == *indent.get_or_insert(line.indent()) {
            found.push(j);
        }
    }
    found
}

fn scan(lines: &[Line]) -> Doc {
    let mask = scalar_mask(lines);
    let mut doc = Doc::default();
    for (i, line) in lines.iter().enumerate() {
        if mask[i] || line.skip() {
            continue;
        }
        let Some((key, value)) = key_value(line.content(), line.start + line.indent()) else { continue };
        let has_value = value.is_some();
        if line.indent() == 0 {
            doc.top_keys.push(key.clone());
            match key.as_str() {
                "apiVersion" => doc.api_version = value,
                "kind" => doc.kind = value,
                "metadata" | "spec" => scan_section(&mut doc, lines, &mask, i, &key, has_value),
                _ => {}
            }
        }
        if key == "env" && !has_value {
            let names = env_names(lines, &mask, i);
            doc.env_lists.push(names);
        }
    }
    doc
}

fn scan_section(doc: &mut Doc, lines: &[Line], mask: &[bool], at: usize, section: &str, inline: bool) {
    if section == "metadata" {
        doc.name_unknown = inline;
    }
    if section == "spec" && inline {
        doc.has_selector = true;
    }
    for j in children(lines, mask, at) {
        let Some((key, value)) = key_value(lines[j].content(), lines[j].start + lines[j].indent()) else { continue };
        match (section, key.as_str()) {
            ("metadata", "name") => doc.name = value,
            ("metadata", "generateName") => doc.has_generate_name = true,
            ("spec", "selector") => doc.has_selector = true,
            _ => {}
        }
    }
}

/// The `name:` of each item of the block list under line `at` (`env:`).
fn env_names(lines: &[Line], mask: &[bool], at: usize) -> Vec<Value> {
    let env_indent = lines[at].indent();
    let mut names = Vec::new();
    let mut item_indent = None;
    let mut key_column = None;
    let mut awaiting_column = false;
    for (j, line) in lines.iter().enumerate().skip(at + 1) {
        if mask[j] || line.skip() {
            continue;
        }
        let (indent, content) = (line.indent(), line.content());
        let is_item = content == "-" || content.starts_with("- ");
        if indent < env_indent || (indent == env_indent && !is_item) {
            break;
        }
        let (key_text, key_at) = if is_item {
            if indent != *item_indent.get_or_insert(indent) {
                continue;
            }
            let inner = content[1..].trim_start();
            if inner.is_empty() {
                (key_column, awaiting_column) = (None, true);
                continue;
            }
            let lead = content.len() - inner.len();
            key_column = Some(indent + lead);
            (inner, line.start + indent + lead)
        } else {
            if awaiting_column {
                (key_column, awaiting_column) = (Some(indent), false);
            }
            if key_column != Some(indent) {
                continue;
            }
            (content, line.start + indent)
        };
        if let Some((key, Some(value))) = key_value(key_text, key_at) {
            if key == "name" {
                names.push(value);
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_documents_and_reads_the_top_level_facts() {
        let text = "---\napiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: a # note\n---\napiVersion: \"apps/v1\"\nkind: Deployment\nspec:\n  selector: {}\n";
        let docs = documents(text);

        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].name.as_ref().unwrap().text, "a");
        assert_eq!(&text[docs[0].name.as_ref().unwrap().start..docs[0].name.as_ref().unwrap().end], "a");
        assert_eq!(docs[1].api_version.as_ref().unwrap().text, "apps/v1");
        assert!(docs[1].has_selector);
        assert_eq!(docs[1].start_line, 6);
    }

    #[test]
    fn block_scalars_hide_their_lines() {
        let text = "apiVersion: v1\nkind: ConfigMap\ndata:\n  script: |\n    env:\n    - name: A\n    - name: A\n";

        assert!(documents(text)[0].env_lists.is_empty());
    }
}
