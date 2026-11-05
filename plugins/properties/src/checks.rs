//! The inspections that need only the file itself: `PropertiesAnnotator`
//! (duplicate keys, broken `\u` escapes), `TrailingSpacesInProperty`,
//! `WrongPropertyKeyValueDelimiter`, `UseEllipsisInProperty` and
//! `AlphaUnsortedPropertiesFile`.

use crate::format::is_sorted;
use crate::props::{Prop, skip_escape, trailing_spaces, trailing_visible_spaces, whole_lines};
use openidea_plugin::{Severity, host};
use std::collections::HashMap;
use std::ops::Range as Span;

/// The plugin's settings: the original's code style and inspection options.
pub struct Settings {
    pub delimiter: char,
    pub spaces_around_delimiter: bool,
    pub keep_blank_lines: bool,
    pub align_delimiters: bool,
    pub ignore_visible_spaces: bool,
    pub ignored_suffixes: Vec<String>,
    pub use_ellipsis: bool,
    pub alpha_unsorted: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            delimiter: '=',
            spaces_around_delimiter: false,
            keep_blank_lines: false,
            align_delimiters: false,
            ignore_visible_spaces: false,
            ignored_suffixes: Vec::new(),
            use_ellipsis: false,
            alpha_unsorted: false,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let defaults = Self::default();
        let flag = |key: &str, default: bool| host::setting_bool(key).unwrap_or(default);

        let delimiter = match host::setting_string("codeStyle.keyValueDelimiter").as_deref() {
            Some(":") => ':',
            Some("space") => ' ',
            _ => '=',
        };

        let ignored_suffixes = host::setting_string("incompleteProperty.ignoredSuffixes")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();

        Self {
            delimiter,
            spaces_around_delimiter: flag("codeStyle.spacesAroundDelimiter", defaults.spaces_around_delimiter),
            keep_blank_lines: flag("codeStyle.keepBlankLines", defaults.keep_blank_lines),
            align_delimiters: flag("codeStyle.alignDelimiters", defaults.align_delimiters),
            ignore_visible_spaces: flag("trailingSpaces.ignoreVisibleSpaces", defaults.ignore_visible_spaces),
            ignored_suffixes,
            use_ellipsis: flag("inspections.useEllipsis", defaults.use_ellipsis),
            alpha_unsorted: flag("inspections.alphaUnsorted", defaults.alpha_unsorted),
        }
    }

    /// The delimiter as written between a new key and its value.
    pub fn delimiter_text(&self) -> String {
        self.delimiter.to_string()
    }
}

pub struct Fix {
    pub title: &'static str,
    pub edits: Vec<(Span<usize>, String)>,
}

pub struct Problem {
    pub span: Span<usize>,
    pub severity: Severity,
    pub message: String,
    pub code: &'static str,
    pub fix: Option<Fix>,
}

pub fn lint(text: &str, props: &[Prop], settings: &Settings) -> Vec<Problem> {
    let mut problems = Vec::new();

    let mut key_counts: HashMap<String, usize> = HashMap::new();
    for p in props {
        *key_counts.entry(p.unescaped_key(text)).or_default() += 1;
    }

    for p in props {
        if key_counts[&p.unescaped_key(text)] > 1 {
            problems.push(Problem {
                span: p.key.clone(),
                severity: Severity::Error,
                message: "Duplicate property key".into(),
                code: "DuplicatePropertyKey",
                fix: Some(Fix { title: "Remove property", edits: vec![(whole_lines(text, p), String::new())] }),
            });
        }

        for span in [&p.key, &p.value] {
            problems.extend(invalid_unicode_escapes(text, span.clone()));
        }

        for span in [&p.key, &p.value] {
            problems.extend(trailing_space_problem(text, span.clone(), settings));
        }

        if p.delimiter(text) != settings.delimiter {
            let edit = match p.sep {
                Some(s) => (s..s + 1, settings.delimiter_text()),
                None => (p.key.end..p.value.start, settings.delimiter_text()),
            };
            problems.push(Problem {
                span: p.span(),
                severity: Severity::Info,
                message: "The key/value delimiter doesn't match the code style setting.".into(),
                code: "WrongPropertyKeyValueDelimiter",
                fix: Some(Fix { title: "Use the code style delimiter", edits: vec![edit] }),
            });
        }

        if settings.use_ellipsis && ends_with_three_dots(&text[p.value.clone()]) {
            let dots = p.value.end - 3..p.value.end;
            problems.push(Problem {
                span: dots.clone(),
                severity: Severity::Warning,
                message: "Three dots instead of an ellipsis (…)".into(),
                code: "UseEllipsisInPropertyInspection",
                fix: Some(Fix { title: "Replace three dots with an ellipsis", edits: vec![(dots, "…".into())] }),
            });
        }
    }

    if settings.alpha_unsorted && !is_sorted(text, props) {
        let first_unsorted = props
            .windows(2)
            .find(|w| text[w[0].key.clone()].to_lowercase() > text[w[1].key.clone()].to_lowercase())
            .map(|w| w[1].key.clone())
            .unwrap_or_default();
        problems.push(Problem {
            span: first_unsorted,
            severity: Severity::Info,
            message: "The properties aren't sorted alphabetically.".into(),
            code: "AlphaUnsortedPropertiesFile",
            fix: None,
        });
    }

    problems
}

fn trailing_space_problem(text: &str, span: Span<usize>, settings: &Settings) -> Option<Problem> {
    if span.is_empty() {
        return None;
    }

    let chars: Vec<(usize, char)> = text[span.clone()].char_indices().collect();
    let only_chars: Vec<char> = chars.iter().map(|&(_, c)| c).collect();
    let from = if settings.ignore_visible_spaces {
        trailing_visible_spaces(&only_chars)
    } else {
        trailing_spaces(&only_chars)
    }?;

    let at = span.start + chars[from].0;
    Some(Problem {
        span: at..span.end,
        severity: Severity::Warning,
        message: "Trailing spaces".into(),
        code: "TrailingSpacesInProperty",
        fix: Some(Fix { title: "Remove trailing spaces", edits: vec![(at..span.end, String::new())] }),
    })
}

/// A `\u` not followed by four hex digits: `java.util.Properties` rejects the file.
fn invalid_unicode_escapes(text: &str, span: Span<usize>) -> Vec<Problem> {
    let b = text.as_bytes();
    let mut problems = Vec::new();
    let mut i = span.start;

    while i < span.end {
        if b[i] != b'\\' {
            i += 1;
            continue;
        }

        if b.get(i + 1) != Some(&b'u') {
            i = skip_escape(b, i);
            continue;
        }

        let digits = b[i + 2..span.end].iter().take(4).take_while(|d| d.is_ascii_hexdigit()).count();
        if digits < 4 {
            problems.push(Problem {
                span: i..i + 2 + digits,
                severity: Severity::Warning,
                message: "Invalid \\u escape: four hex digits must follow \\u.".into(),
                code: "InvalidUnicodeEscape",
                fix: Some(Fix { title: "Unescape", edits: vec![(i..i + 1, String::new())] }),
            });
        }
        i += 2 + digits;
    }

    problems
}

/// Ends with exactly three dots, after at least two other characters.
fn ends_with_three_dots(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    chars.len() > 4 && chars.ends_with(&['.', '.', '.']) && chars[chars.len() - 4] != '.'
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::props::parse;

    fn codes(text: &str, settings: &Settings) -> Vec<(&'static str, String)> {
        lint(text, &parse(text), settings).into_iter().map(|p| (p.code, text[p.span].to_owned())).collect()
    }

    #[test]
    fn reports_file_problems() {
        let settings = Settings::default();
        assert_eq!(
            codes("a=1\na=2\nb:3\nc=x  \nd=\\u12g\n", &settings),
            [
                ("DuplicatePropertyKey", "a".to_owned()),
                ("DuplicatePropertyKey", "a".to_owned()),
                ("WrongPropertyKeyValueDelimiter", "b:3".to_owned()),
                ("TrailingSpacesInProperty", "  ".to_owned()),
                ("InvalidUnicodeEscape", "\\u12".to_owned()),
            ]
        );
        assert!(codes("ok=1\nescaped=\\\\u12\n", &settings).is_empty());
    }

    #[test]
    fn opt_in_inspections_stay_quiet_by_default() {
        let text = "b=Loading...\na=x\n";
        assert!(codes(text, &Settings::default()).is_empty());

        let settings = Settings { use_ellipsis: true, alpha_unsorted: true, ..Settings::default() };
        assert_eq!(
            codes(text, &settings),
            [("UseEllipsisInPropertyInspection", "...".to_owned()), ("AlphaUnsortedPropertiesFile", "a".to_owned())]
        );
        assert!(codes("a=Wait....\n", &settings).is_empty());
    }
}
