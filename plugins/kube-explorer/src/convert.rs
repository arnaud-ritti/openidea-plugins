//! Intentions on a manifest file: turn a `kind: List` into separate documents
//! and several documents into one `kind: List`. Both rewrite only the open
//! file and copy the item text line by line, so comments survive.

use openidea_plugin::providers::{CodeAction, Document};
use openidea_plugin::text::{offset, position};
use openidea_plugin::{Range, TextEdit};

use crate::checks::is_helm_template;
use crate::manifest::{Doc, documents};

pub const LIST_TO_DOCUMENTS: &str = "Convert List to separate documents";
pub const DOCUMENTS_TO_LIST: &str = "Convert documents to a single List";

pub fn code_actions(doc: &Document, selection: Range) -> Vec<CodeAction> {
    let text = &doc.text;
    if !text.contains("apiVersion:") || is_helm_template(text) || has_anchors(text) {
        return Vec::new();
    }
    let docs = documents(text);
    let caret = offset(text, selection.start).unwrap_or(0);
    let mut edits = Vec::new();

    for list in docs.iter().filter(|d| d.kind.as_ref().is_some_and(|k| k.text == "List") && d.range.start <= caret && caret <= d.range.end) {
        if let Some(replacement) = list_to_documents(&text[list.range.clone()]) {
            edits.push((LIST_TO_DOCUMENTS, list.range.clone(), replacement));
        }
    }
    if let Some(replacement) = documents_to_list(text, &docs) {
        edits.push((DOCUMENTS_TO_LIST, 0..text.len(), replacement));
    }

    edits
        .into_iter()
        .map(|(title, range, text_after)| CodeAction {
            title: title.into(),
            kind: Some("refactor.rewrite".into()),
            preferred: false,
            edits: vec![(
                doc.path.clone(),
                vec![TextEdit { range: Range::new(position(text, range.start), position(text, range.end)), text: text_after }],
            )],
            data: None,
        })
        .collect()
}

fn eol_of(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

fn is_dash(line: &str) -> bool {
    let content = line.trim_start();
    content == "-" || content.starts_with("- ")
}

/// A `kind: List` document body as one document per item, joined by `---`.
fn list_to_documents(body: &str) -> Option<String> {
    let eol = eol_of(body);
    let lines: Vec<&str> = body.lines().collect();
    let top: Vec<usize> = (0..lines.len()).filter(|&i| indent(lines[i]) == 0 && !is_blank(lines[i]) && !is_comment(lines[i])).collect();
    let key_of = |i: usize| lines[i].split(':').next().unwrap_or_default().trim();
    let items_at = *top.iter().find(|&&i| key_of(i) == "items" && lines[i].trim_end() == "items:")?;
    if top.iter().any(|&i| !is_dash(lines[i]) && !["apiVersion", "kind", "metadata", "items"].contains(&key_of(i))) {
        return None;
    }

    let header: Vec<&str> = lines[..top[0]].to_vec();
    let items_end = top.iter().copied().find(|&i| i > items_at && !is_dash(lines[i])).unwrap_or(lines.len());
    let region = &lines[items_at + 1..items_end];
    let first = region.iter().position(|l| !is_blank(l) && !is_comment(l))?;
    let item_indent = indent(region[first]);
    let starts: Vec<usize> = (first..region.len()).filter(|&i| indent(region[i]) == item_indent && is_dash(region[i])).collect();
    if starts.is_empty() {
        return None;
    }

    let mut carried: Vec<&str> = Vec::new();
    let mut documents_out: Vec<String> = Vec::new();
    for (n, &start) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(region.len());
        let mut chunk: Vec<&str> = region[start..end].to_vec();
        let mut next_head: Vec<&str> = Vec::new();
        if n + 1 < starts.len() {
            while let Some(&last) = chunk.last() {
                if is_blank(last) || (is_comment(last) && indent(last) <= item_indent) {
                    chunk.pop();
                    if !is_blank(last) {
                        next_head.insert(0, last);
                    }
                    continue;
                }
                break;
            }
        }
        let dash_rest = chunk[0].trim_start()[1..].trim_start();
        if dash_rest.is_empty() {
            return None;
        }
        let width = chunk[0].len() - dash_rest.len();
        let mut item: Vec<String> = carried.iter().map(|l| l.trim_start().to_owned()).collect();
        item.push(dash_rest.to_owned());
        for line in &chunk[1..] {
            if is_blank(line) {
                item.push(String::new());
                continue;
            }
            if indent(line) < width && !is_comment(line) {
                return None;
            }
            item.push(line[indent(line).min(width)..].to_owned());
        }
        carried = next_head;

        let body_text = item.join(eol);
        let scanned = documents(&body_text);
        if scanned.len() != 1 || scanned[0].top_keys.is_empty() {
            return None;
        }
        documents_out.push(body_text);
    }

    let mut result: String = header.iter().map(|l| format!("{l}{eol}")).collect();
    result.push_str(&documents_out.join(&format!("{eol}---{eol}")));
    if body.ends_with('\n') {
        result.push_str(eol);
    }
    Some(result)
}

/// Several manifest documents as the items of one `kind: List`.
fn documents_to_list(text: &str, docs: &[Doc]) -> Option<String> {
    let eol = eol_of(text);
    let filled: Vec<&Doc> = docs.iter().filter(|d| !text[d.range.clone()].trim().is_empty()).collect();
    let single_manifests = filled.iter().all(|d| d.is_manifest() && d.kind.as_ref().is_some_and(|k| !k.text.ends_with("List")));
    if filled.len() < 2 || !single_manifests || text.lines().any(|l| l.starts_with(['%', '.'])) {
        return None;
    }

    let mut result = format!("apiVersion: v1{eol}kind: List{eol}items:{eol}");
    for doc in filled {
        let mut dashed = false;
        let body: Vec<&str> = text[doc.range.clone()].lines().collect();
        let first = body.iter().position(|l| !is_blank(l))?;
        let last = body.iter().rposition(|l| !is_blank(l))?;
        for line in &body[first..=last] {
            let prefix = match (is_blank(line), is_comment(line), dashed) {
                (true, ..) => "",
                (_, true, false) => "  ",
                (_, _, false) => "  - ",
                _ => "    ",
            };
            dashed |= !is_blank(line) && !is_comment(line);
            result.push_str(&format!("{prefix}{line}{eol}"));
        }
    }
    if !text.ends_with('\n') {
        result.truncate(result.len() - eol.len());
    }
    Some(result)
}

/// Whether the text uses anchors, aliases or merge keys, which a copy of its lines could break.
pub fn has_anchors(text: &str) -> bool {
    text.lines().any(|line| {
        let content = line.trim_start();
        content.starts_with("<<:")
            || line.char_indices().any(|(i, c)| {
                matches!(c, '&' | '*')
                    && (i == 0 || line[..i].ends_with([' ', '[', '{', ',']))
                    && line[i + 1..].starts_with(|n: char| n.is_alphanumeric() || n == '_' || n == '-')
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KubeExplorer;
    use openidea_plugin::Position;
    use openidea_plugin::providers::Providers;

    fn doc(text: &str) -> Document {
        Document { path: "/p/a.yaml".into(), language: "yaml".into(), version: 1, text: text.into() }
    }

    fn at(line: u32) -> Range {
        Range::new(Position::new(line, 0), Position::new(line, 0))
    }

    fn convert(text: &str, title: &str, line: u32) -> Option<String> {
        let actions = code_actions(&doc(text), at(line));
        let action = actions.iter().find(|a| a.title == title)?;
        let mut result = text.to_owned();
        for edit in action.edits[0].1.iter().rev() {
            let (from, to) = (offset(&result, edit.range.start).unwrap(), offset(&result, edit.range.end).unwrap());
            result.replace_range(from..to, &edit.text);
        }
        Some(result)
    }

    const LIST: &str = "# header\napiVersion: v1\nkind: List\nitems:\n  - apiVersion: v1\n    kind: ConfigMap # keep\n    metadata:\n      name: a\n    data:\n      k: |\n        line\n\n  # second\n  - apiVersion: v1\n    kind: Pod\n    metadata:\n      name: b\n";
    const SPLIT: &str = "# header\napiVersion: v1\nkind: ConfigMap # keep\nmetadata:\n  name: a\ndata:\n  k: |\n    line\n---\n# second\napiVersion: v1\nkind: Pod\nmetadata:\n  name: b\n";

    #[test]
    fn list_becomes_separate_documents() {
        assert_eq!(convert(LIST, LIST_TO_DOCUMENTS, 4).unwrap(), SPLIT);
    }

    #[test]
    fn list_with_items_at_the_key_indent_and_crlf_converts() {
        let text = "apiVersion: v1\nkind: List\nitems:\n- apiVersion: v1\n  kind: Pod\n  metadata:\n    name: a\n- apiVersion: v1\n  kind: Pod\n  metadata:\n    name: b\nmetadata: {}\n".replace('\n', "\r\n");
        let expected = "apiVersion: v1\nkind: Pod\nmetadata:\n  name: a\n---\napiVersion: v1\nkind: Pod\nmetadata:\n  name: b\n".replace('\n', "\r\n");
        assert_eq!(convert(&text, LIST_TO_DOCUMENTS, 2).unwrap(), expected);
    }

    #[test]
    fn documents_become_list() {
        let text = "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: a # c\n---\n# pod\napiVersion: v1\nkind: Pod\nmetadata:\n  name: b\n---\n";
        let expected = "apiVersion: v1\nkind: List\nitems:\n  - apiVersion: v1\n    kind: ConfigMap\n    metadata:\n      name: a # c\n  # pod\n  - apiVersion: v1\n    kind: Pod\n    metadata:\n      name: b\n";
        assert_eq!(convert(text, DOCUMENTS_TO_LIST, 0).unwrap(), expected);
    }

    #[test]
    fn list_and_documents_round_trip() {
        let list = convert(SPLIT, DOCUMENTS_TO_LIST, 0).unwrap();
        let back = convert(&list, LIST_TO_DOCUMENTS, 3).unwrap();
        assert_eq!(documents(&back).len(), 2);
        assert!(back.contains("kind: ConfigMap # keep") && back.contains("# second"));
    }

    #[test]
    fn list_to_documents_needs_the_caret_in_the_list() {
        let text = format!("apiVersion: v1\nkind: Pod\nmetadata:\n  name: x\n---\n{}", LIST.replace("# header\n", ""));
        assert!(convert(&text, LIST_TO_DOCUMENTS, 1).is_none());
        assert!(convert(&text, LIST_TO_DOCUMENTS, 8).is_some());
    }

    #[test]
    fn intentions_are_quiet_for_library_or_dynamic_input() {
        let anchors = "apiVersion: v1\nkind: List\nitems:\n  - &a\n    apiVersion: v1\n    kind: Pod\n  - *a\n";
        let helm = LIST.replace("name: a", "name: {{ .Values.a }}");
        let empty = "apiVersion: v1\nkind: List\nitems: []\n";
        let flow = "apiVersion: v1\nkind: List\nitems:\n  - {apiVersion: v1, kind: Pod}\n";
        let extra = LIST.replace("items:", "foo: bar\nitems:");
        for text in [anchors, helm.as_str(), empty, flow, extra.as_str()] {
            assert!(convert(text, LIST_TO_DOCUMENTS, 2).is_none(), "{text}");
        }
        let two = "apiVersion: v1\nkind: Pod\nmetadata:\n  name: a\n---\napiVersion: v1\nkind: Pod\nmetadata:\n  name: b\n";
        let alias = two.replace("name: b", "name: &n b");
        let helm_docs = two.replace("name: b", "name: {{ .Values.b }}");
        let comment_only = format!("{two}---\n# just a comment\n");
        let nested = format!("{two}---\napiVersion: v1\nkind: List\nitems: []\n");
        let other = format!("{two}---\nname: not-a-manifest\n");
        for text in [alias.as_str(), helm_docs.as_str(), comment_only.as_str(), nested.as_str(), other.as_str(), &two[..45]] {
            assert!(convert(text, DOCUMENTS_TO_LIST, 0).is_none(), "{text}");
        }
        assert!(convert(two, DOCUMENTS_TO_LIST, 0).is_some());
    }

    #[test]
    fn yaml_provider_ignores_unrelated_files() {
        for text in ["services:\n  web:\n    image: nginx\n", "stages: [build]\n", "name: a\n---\nname: b\n", ""] {
            assert!(KubeExplorer::default().code_actions(&doc(text), at(0)).is_empty(), "{text}");
        }
    }

    #[test]
    fn provider_offers_schema_and_conversion_together() {
        let actions = KubeExplorer::default().code_actions(&doc("apiVersion: v1\nkind: List\nitems:\n  - apiVersion: v1\n    kind: Pod\n"), at(2));
        let titles: Vec<_> = actions.iter().map(|a| a.title.as_str()).collect();
        assert!(titles.contains(&LIST_TO_DOCUMENTS), "{titles:?}");
    }

    #[test]
    fn ranges_use_utf16_columns_with_multibyte_text() {
        let text = "apiVersion: v1\nkind: List\nitems:\n  - apiVersion: v1\n    kind: Pod\n    metadata: {name: \"é😀\"}\n";
        let action = &code_actions(&doc(text), at(1))[0];
        let edit = &action.edits[0].1[0];
        assert_eq!((edit.range.start.line, edit.range.end.line), (0, 6));
        assert!(edit.text.contains("é😀"));
    }
}
