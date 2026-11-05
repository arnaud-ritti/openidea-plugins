//! The original plugin's inspections, with their quick fixes.

use crate::parse::{Kind, Property, Token};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fix {
    pub title: &'static str,
    pub range: Range<usize>,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The original inspection's short name.
    pub code: &'static str,
    pub level: Level,
    pub range: Range<usize>,
    pub message: String,
    pub fix: Option<Fix>,
}

impl Problem {
    fn warning(code: &'static str, range: Range<usize>, message: impl Into<String>) -> Self {
        Self { code, level: Level::Warning, range, message: message.into(), fix: None }
    }

    fn fix(mut self, title: &'static str, range: Range<usize>, text: impl Into<String>) -> Self {
        self.fix = Some(Fix { title, range, text: text.into() });
        self
    }
}

/// `defined` tells whether a key is declared anywhere (for `${KEY}`).
pub fn check(text: &str, tokens: &[Token], properties: &[Property], defined: impl Fn(&str) -> bool) -> Vec<Problem> {
    let mut problems = Vec::new();
    let doubles = properties.iter().filter(|p| p.quote == Some('"')).count();
    let singles = properties.iter().filter(|p| p.quote == Some('\'')).count();
    let quote = if doubles > singles { '"' } else { '\'' };
    let mut seen: HashMap<String, (Range<usize>, bool)> = HashMap::new();

    for property in properties {
        let key = &text[property.key.clone()];

        if key.contains('-') {
            problems.push(Problem::warning(
                "DotEnvIncorrectDelimiterInspection",
                property.key.clone(),
                "Key contains '-'. Use '_' instead.",
            ));
        }

        if !key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_' || c == '-') {
            problems.push(Problem::warning(
                "DotEnvLeadingCharacterInspection",
                property.key.clone(),
                "Key must start with a letter or '_'.",
            ));
        }

        if key.bytes().any(|b| b.is_ascii_lowercase()) {
            problems.push(Problem::warning(
                "DotEnvLowercaseKeyInspection",
                property.key.clone(),
                "Key contains lowercase letters. Use uppercase letters only.",
            ));
        }

        match seen.entry(property.name(text)) {
            Entry::Vacant(first) => {
                first.insert((property.key.clone(), false));
            }
            Entry::Occupied(mut first) => {
                let message = "Key is declared more than once in this file.";
                problems.push(Problem::warning("DotEnvDuplicateKeyInspection", property.key.clone(), message));
                let (range, marked) = first.get_mut();
                if !*marked {
                    problems.push(Problem::warning("DotEnvDuplicateKeyInspection", range.clone(), message));
                    *marked = true;
                }
            }
        }

        if let Some(separator) = &property.separator {
            let around = property.key.end..property.value.as_ref().map_or(separator.end, |v| v.start);
            let spaced = &text[around.clone()];
            if &text[separator.clone()] == "=" && spaced != "=" && !spaced.contains(['\n', '\r']) {
                problems.push(
                    Problem::warning("DotEnvSpaceAroundSeparatorInspection", around.clone(), "Spaces around '=' are not allowed.")
                        .fix("Remove spaces around '='", around, "="),
                );
            }
        }

        let Some(value) = property.value.clone() else { continue };
        let raw = &text[value.clone()];
        let trimmed_end = value.start + raw.trim_end().len();

        if property.quote.is_none() && raw.trim().contains(' ') {
            problems.push(
                Problem::warning(
                    "DotEnvSpaceInsideNonQuotedInspection",
                    value.start..trimmed_end,
                    "Values with spaces must be quoted.",
                )
                .fix("Add quotes", value.start..trimmed_end, format!("{quote}{}{quote}", raw.trim_end())),
            );
        }

        if !raw.contains(['\n', '\r']) && raw.ends_with([' ', '\t']) {
            let spaces = value.start + raw.trim_end_matches([' ', '\t']).len()..value.end;
            problems.push(
                Problem::warning("DotEnvTrailingWhitespaceInspection", spaces.clone(), "Line has trailing whitespace.")
                    .fix("Remove trailing whitespace", spaces, ""),
            );
        }

        if raw.contains("${") {
            let inner = raw.trim();
            let start = value.start + (raw.len() - raw.trim_start().len());
            if !(inner.starts_with('"') && inner.ends_with('"')) {
                let range = start..start + inner.len();
                problems.push(Problem {
                    code: "NestedVariableOutsideDoubleQuotesInspection",
                    level: Level::Error,
                    range: range.clone(),
                    message: "Values containing ${…} must be in double quotes.".into(),
                    fix: Some(Fix { title: "Put the value in double quotes", range, text: format!("\"{inner}\"") }),
                });
            }
        }
    }

    for (i, token) in tokens.iter().enumerate() {
        match token.kind {
            Kind::Space => {
                let blank = &text[token.range.clone()];
                if blank.matches('\n').count() > 2 {
                    let newline = if blank.contains("\r\n") { "\r\n" } else { "\n" };
                    problems.push(
                        Problem::warning(
                            "DotEnvExtraBlankLineInspection",
                            token.range.clone(),
                            "Only one blank line is allowed between keys.",
                        )
                        .fix("Remove extra blank lines", token.range.clone(), newline.repeat(2)),
                    );
                }

                let mut line_start = token.range.start;
                for (offset, _) in blank.match_indices('\n') {
                    let newline_at = token.range.start + offset;
                    let line_end = if text[..newline_at].ends_with('\r') { newline_at - 1 } else { newline_at };
                    let kept = line_start + text[line_start..line_end].trim_end_matches([' ', '\t']).len();
                    if kept < line_end {
                        problems.push(
                            Problem::warning("DotEnvTrailingWhitespaceInspection", kept..line_end, "Line has trailing whitespace.")
                                .fix("Remove trailing whitespace", kept..line_end, ""),
                        );
                    }
                    line_start = newline_at + 1;
                }
            }
            Kind::NestedKey => {
                let key = &text[token.range.clone()];
                if !defined(key) {
                    problems.push(Problem::warning(
                        "UndefinedNestedVariableInspection",
                        token.range.clone(),
                        format!("Variable '{key}' is not declared in any .env file, Dockerfile or compose file."),
                    ));
                }
            }
            Kind::NestedStart => {
                if let Some(end) = tokens.get(i + 1).filter(|t| t.kind == Kind::NestedEnd) {
                    problems.push(Problem::warning(
                        "EmptyNestedVariableInspection",
                        token.range.start..end.range.end,
                        "${} has no variable name.",
                    ));
                }
            }
            _ => {}
        }
    }

    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{lex, properties};

    fn run(text: &str) -> Vec<(&'static str, String)> {
        let tokens = lex(text);
        let props = properties(text, &tokens);
        check(text, &tokens, &props, |k| k == "KNOWN")
            .into_iter()
            .map(|p| (p.code, text[p.range].to_owned()))
            .collect()
    }

    fn fixed(text: &str, code: &str) -> String {
        let tokens = lex(text);
        let props = properties(text, &tokens);
        let fix = check(text, &tokens, &props, |_| true).into_iter().find(|p| p.code == code).unwrap().fix.unwrap();
        let mut out = text.to_owned();
        out.replace_range(fix.range, &fix.text);
        out
    }

    #[test]
    fn a_clean_file_has_no_problems() {
        assert_eq!(run("# comment\nexport APP_NAME=app\n\nURL=\"http://${KNOWN}/x\"\nEMPTY=\n"), []);
    }

    #[test]
    fn reports_key_problems() {
        assert_eq!(
            run("1A=x\nmy-key=y\nDUP=1\nDUP=2\nDUP=3\n"),
            [
                ("DotEnvLeadingCharacterInspection", "1A".into()),
                ("DotEnvIncorrectDelimiterInspection", "my-key".into()),
                ("DotEnvLowercaseKeyInspection", "my-key".into()),
                ("DotEnvDuplicateKeyInspection", "DUP".into()),
                ("DotEnvDuplicateKeyInspection", "DUP".into()),
                ("DotEnvDuplicateKeyInspection", "DUP".into()),
            ]
        );
    }

    #[test]
    fn reports_value_and_layout_problems() {
        assert_eq!(
            run("A = b c\nB=${X}\nC=\"${}\"\n\n\n\nD=\"${KNOWN}\"   \n"),
            [
                ("DotEnvSpaceAroundSeparatorInspection", " = ".into()),
                ("DotEnvSpaceInsideNonQuotedInspection", "b c".into()),
                ("NestedVariableOutsideDoubleQuotesInspection", "${X}".into()),
                ("EmptyNestedVariableInspection", "${}".into()),
                ("DotEnvExtraBlankLineInspection", "\n\n\n\n".into()),
                ("DotEnvTrailingWhitespaceInspection", "   ".into()),
            ]
        );
        assert_eq!(run("A=\"${MISSING}\"")[0], ("UndefinedNestedVariableInspection", "MISSING".into()));
        assert_eq!(run("A=1  \nB  \n"), [
            ("DotEnvTrailingWhitespaceInspection", "  ".into()),
            ("DotEnvTrailingWhitespaceInspection", "  ".into()),
        ]);
    }

    #[test]
    fn fixes() {
        assert_eq!(fixed("A = b\n", "DotEnvSpaceAroundSeparatorInspection"), "A=b\n");
        assert_eq!(fixed("A=b c\nB=\"x\"\n", "DotEnvSpaceInsideNonQuotedInspection"), "A=\"b c\"\nB=\"x\"\n");
        assert_eq!(fixed("A=b c\n", "DotEnvSpaceInsideNonQuotedInspection"), "A='b c'\n");
        assert_eq!(fixed("A=1 \n", "DotEnvTrailingWhitespaceInspection"), "A=1\n");
        assert_eq!(fixed("A=1\n\n\n\nB=2", "DotEnvExtraBlankLineInspection"), "A=1\n\nB=2");
        assert_eq!(fixed("A='${B}'", "NestedVariableOutsideDoubleQuotesInspection"), "A=\"'${B}'\"");
    }
}
