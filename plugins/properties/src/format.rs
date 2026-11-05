//! `PropertiesFormattingModelBuilder` (Reformat) and the sorting of
//! `AlphaUnsortedPropertiesFileInspection`'s quick fix.

use crate::checks::Settings;
use crate::props::{Item, Prop, items};
use std::ops::Range as Span;

fn line_break(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// The edits that format `text` like the original formatter: entries start
/// their line, blank lines between them go unless kept, one space (or none)
/// around the delimiter, delimiters optionally aligned across the file.
pub fn reformat_edits(text: &str, settings: &Settings) -> Vec<(Span<usize>, String)> {
    let eol = line_break(text);
    let items = items(text);
    let mut edits = Vec::new();
    let mut replace = |span: Span<usize>, new: String| {
        if text[span.clone()] != new {
            edits.push((span, new));
        }
    };

    let mut previous_end = 0;
    for (n, item) in items.iter().enumerate() {
        let span = item.span();
        let breaks = text[previous_end..span.start].matches('\n').count();
        let kept = if n == 0 || settings.keep_blank_lines { breaks } else { breaks.min(1) };
        replace(previous_end..span.start, eol.repeat(if n == 0 { kept } else { kept.max(1) }));
        previous_end = span.end;
    }

    let props: Vec<&Prop> = items
        .iter()
        .filter_map(|item| match item {
            Item::Prop(p) if p.sep.is_some() || !p.value.is_empty() => Some(p),
            _ => None,
        })
        .collect();
    let width = |p: &Prop| text[p.key.clone()].chars().count();
    let min_gap = |p: &Prop| if p.sep.is_some() { usize::from(settings.spaces_around_delimiter) } else { 1 };
    let column = props.iter().map(|p| width(p) + min_gap(p)).max().unwrap_or(0);

    for p in props {
        let gap = if settings.align_delimiters { column - width(p) } else { min_gap(p) };
        match p.sep {
            Some(sep) => {
                replace(p.key.end..sep, " ".repeat(gap));
                let after = if settings.spaces_around_delimiter && !p.value.is_empty() { " " } else { "" };
                replace(sep + 1..p.value.start, after.into());
            }
            None => replace(p.key.end..p.value.start, " ".repeat(gap)),
        }
    }

    edits.sort_by_key(|(span, _)| span.start);
    edits
}

/// Keys in case-insensitive order (`String.CASE_INSENSITIVE_ORDER`).
pub fn is_sorted(text: &str, props: &[Prop]) -> bool {
    props.windows(2).all(|w| text[w[0].key.clone()].to_lowercase() <= text[w[1].key.clone()].to_lowercase())
}

/// The file with its properties sorted by key. A comment block at the top
/// that a blank line separates from the rest stays there; other comments
/// move with the property below them.
pub fn sorted_text(text: &str, settings: &Settings) -> String {
    let eol = line_break(text);
    let items = items(text);
    let Some(first_prop) = items.iter().position(|i| matches!(i, Item::Prop(_))) else {
        return text.to_owned();
    };

    let header_len = (0..first_prop)
        .rev()
        .find(|&i| text[items[i].span().end..items[i + 1].span().start].matches('\n').count() > 1)
        .map_or(0, |i| i + 1);

    let mut entries: Vec<(String, String)> = Vec::new();
    let mut comments: Vec<&str> = Vec::new();
    for item in &items[header_len..] {
        match item {
            Item::Comment(span) => comments.push(&text[span.clone()]),
            Item::Prop(p) => {
                let mut entry = comments.iter().map(|c| format!("{c}{eol}")).collect::<String>();
                entry.push_str(&format!("{}{}{}", &text[p.key.clone()], settings.delimiter_text(), &text[p.value.clone()]));
                entries.push((text[p.key.clone()].to_lowercase(), entry));
                comments.clear();
            }
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut blocks: Vec<String> = Vec::new();
    if header_len > 0 {
        let header: Vec<&str> = items[..header_len].iter().map(|i| &text[i.span()]).collect();
        blocks.push(format!("{}{eol}", header.join(eol)));
    }
    blocks.extend(entries.into_iter().map(|(_, entry)| entry));
    blocks.extend(comments.iter().map(|c| (*c).to_owned()));

    let mut out = blocks.join(eol);
    if text.ends_with('\n') {
        out.push_str(eol);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reformat(text: &str, settings: &Settings) -> String {
        let mut out = text.to_owned();
        for (span, new) in reformat_edits(text, settings).into_iter().rev() {
            out.replace_range(span, &new);
        }
        out
    }

    #[test]
    fn reformats_with_the_default_code_style() {
        assert_eq!(reformat("  a = 1\n\n\n# c\nlong.key:2\nk   v\n", &Settings::default()), "a=1\n# c\nlong.key:2\nk v\n");
    }

    #[test]
    fn reformats_with_spaces_kept_lines_and_alignment() {
        let settings = Settings { spaces_around_delimiter: true, keep_blank_lines: true, align_delimiters: true, ..Settings::default() };
        assert_eq!(reformat("a=1\n\nlong.key=2\nk v\n", &settings), "a        = 1\n\nlong.key = 2\nk        v\n");
    }

    #[test]
    fn sorts_keeping_header_and_attached_comments() {
        let text = "# header\n\n# about b\nb=2\nA:1\n# trailing\n";
        assert_eq!(sorted_text(text, &Settings::default()), "# header\n\nA=1\n# about b\nb=2\n# trailing\n");
        assert!(is_sorted("a=1\nB=2\n", &crate::props::parse("a=1\nB=2\n")));
    }
}
