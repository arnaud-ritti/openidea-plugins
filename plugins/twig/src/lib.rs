//! Twig for OpenIDEA: highlighting, structure view (blocks, macros) and
//! folding (tag pairs, comments) from a hand-written lexer, exported as the
//! plugin's `syntax-provider`. Detection comes from `languages/twig.toml`.

use std::ops::Range;

use openidea_plugin::syntax::{Fold, FoldKind, Highlight, Symbol, SymbolKind, SyntaxProvider, Token};
use openidea_plugin::{Error, Invocation, Plugin, register_plugin};

#[derive(Default)]
pub struct Twig;

impl Plugin for Twig {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

impl SyntaxProvider for Twig {
    fn highlights(&mut self, _language: &str, text: &str) -> Vec<Token> {
        lex(text).tokens
    }

    fn outline(&mut self, _language: &str, text: &str) -> Vec<Symbol> {
        structure(text).0
    }

    fn folding(&mut self, _language: &str, text: &str) -> Vec<Fold> {
        structure(text).1
    }
}

register_plugin!(Twig, syntax);

const KEYWORDS: &[&str] = &[
    "and", "or", "not", "in", "is", "matches", "starts", "ends", "with", "as", "only", "ignore", "missing", "if",
    "else", "for",
];
const CONSTANTS: &[&str] = &["true", "false", "null", "none", "TRUE", "FALSE", "NULL", "NONE"];

/// A `{% name … %}` tag: its name, the identifier after it (a block's or
/// macro's name), and where it is.
#[derive(Debug, PartialEq, Eq)]
struct Tag {
    name: String,
    argument: Option<Range<usize>>,
    range: Range<usize>,
    /// Has an `{% endname %}`: not `{% block title 'Short' %}` or `{% set x = 1 %}`.
    paired: bool,
}

#[derive(Default)]
struct Lexed {
    tokens: Vec<Token>,
    tags: Vec<Tag>,
    comments: Vec<Range<usize>>,
}

impl Lexed {
    fn push(&mut self, range: Range<usize>, kind: Highlight) {
        if range.start < range.end {
            self.tokens.push(Token { range, kind });
        }
    }
}

struct Lexer<'a> {
    text: &'a str,
    bytes: &'a [u8],
    out: Lexed,
}

fn is_identifier_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_identifier(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn lex(text: &str) -> Lexed {
    let mut lexer = Lexer { text, bytes: text.as_bytes(), out: Lexed::default() };
    lexer.html(0);
    lexer.out
}

impl Lexer<'_> {
    fn at(&self, i: usize, s: &str) -> bool {
        self.bytes[i..].starts_with(s.as_bytes())
    }

    fn find(&self, from: usize, s: &str) -> Option<usize> {
        self.text[from..].find(s).map(|i| from + i)
    }

    fn html(&mut self, mut i: usize) {
        while i < self.bytes.len() {
            if self.at(i, "{#") || self.at(i, "{{") || self.at(i, "{%") {
                i = self.twig(i);
                continue;
            }

            if self.at(i, "<!--") {
                let end = self.find(i + 4, "-->").map_or(self.bytes.len(), |e| e + 3);
                self.out.push(i..end, Highlight::Comment);
                i = end;
                continue;
            }

            let opens_element = self.bytes[i] == b'<'
                && self.bytes.get(i + 1).is_some_and(|b| is_identifier_start(*b) || *b == b'/' || *b == b'!');
            if opens_element {
                i = self.element(i);
                continue;
            }

            i += 1;
        }
    }

    /// `<name attr="value" …>`, with Twig anywhere in it.
    fn element(&mut self, start: usize) -> usize {
        let mut i = start + 1;
        while i < self.bytes.len() && (is_identifier(self.bytes[i]) || matches!(self.bytes[i], b'/' | b'!' | b'-' | b':')) {
            i += 1;
        }
        self.out.push(start..i, Highlight::Tag);

        while i < self.bytes.len() {
            let b = self.bytes[i];

            if self.at(i, "{#") || self.at(i, "{{") || self.at(i, "{%") {
                i = self.twig(i);
            } else if b == b'>' {
                self.out.push(i..i + 1, Highlight::Tag);
                return i + 1;
            } else if self.at(i, "/>") {
                self.out.push(i..i + 2, Highlight::Tag);
                return i + 2;
            } else if b == b'"' || b == b'\'' {
                i = self.attribute_value(i);
            } else if b.is_ascii_whitespace() || b == b'=' {
                i += 1;
            } else if b == b'<' {
                return i;
            } else {
                let name_start = i;
                while i < self.bytes.len() && !matches!(self.bytes[i], b' ' | b'\t' | b'\n' | b'\r' | b'=' | b'>' | b'/' | b'"' | b'\'' | b'{' | b'<') {
                    i += 1;
                }
                i = i.max(name_start + 1);
                self.out.push(name_start..i, Highlight::Attribute);
            }
        }

        i
    }

    fn attribute_value(&mut self, start: usize) -> usize {
        let quote = self.bytes[start];
        let mut segment = start;
        let mut i = start + 1;

        while i < self.bytes.len() {
            if self.bytes[i] == quote {
                self.out.push(segment..i + 1, Highlight::String);
                return i + 1;
            }

            if self.at(i, "{{") || self.at(i, "{%") || self.at(i, "{#") {
                self.out.push(segment..i, Highlight::String);
                i = self.twig(i);
                segment = i;
                continue;
            }

            i += 1;
        }

        self.out.push(segment..i, Highlight::String);
        i
    }

    /// A Twig comment, print or tag starting at `start`; returns its end.
    fn twig(&mut self, start: usize) -> usize {
        if self.at(start, "{#") {
            let end = self.find(start + 2, "#}").map_or(self.bytes.len(), |e| e + 2);
            self.out.push(start..end, Highlight::Comment);
            self.out.comments.push(start..end);
            return end;
        }

        let is_tag = self.at(start, "{%");
        let close = if is_tag { "%}" } else { "}}" };
        let mut i = start + 2;
        if self.bytes.get(i).is_some_and(|b| *b == b'-' || *b == b'~') {
            i += 1;
        }
        self.out.push(start..i, Highlight::Punctuation);

        let mut tag_name: Option<String> = None;
        let mut argument = None;
        let mut token_count = 0;
        let mut assigns = false;
        let mut previous = 0u8;

        while i < self.bytes.len() {
            let b = self.bytes[i];

            let trimmed_close = (b == b'-' || b == b'~') && self.at(i + 1, close);
            if self.at(i, close) || trimmed_close {
                let end = i + close.len() + usize::from(trimmed_close);
                self.out.push(i..end, Highlight::Punctuation);
                i = end;
                break;
            }

            if b.is_ascii_whitespace() {
                i += 1;
                continue;
            }

            let token_start = i;
            token_count += 1;
            if b == b'"' || b == b'\'' {
                i += 1;
                while i < self.bytes.len() && self.bytes[i] != b {
                    i += if self.bytes[i] == b'\\' { 2 } else { 1 };
                }
                i = (i + 1).min(self.bytes.len());
                self.out.push(token_start..i, Highlight::String);
            } else if b.is_ascii_digit() {
                while i < self.bytes.len() && (self.bytes[i].is_ascii_digit() || self.bytes[i] == b'.' || self.bytes[i] == b'_') {
                    i += 1;
                }
                self.out.push(token_start..i, Highlight::Number);
            } else if is_identifier_start(b) {
                while i < self.bytes.len() && is_identifier(self.bytes[i]) {
                    i += 1;
                }
                let word = &self.text[token_start..i];
                let next = self.text[i..].trim_start().bytes().next();
                let is_tag_name = is_tag && tag_name.is_none();

                if is_tag && !is_tag_name && argument.is_none() {
                    argument = Some(token_start..i);
                }

                let kind = if is_tag_name {
                    tag_name = Some(word.to_owned());
                    Highlight::Keyword
                } else if previous == b'|' {
                    Highlight::Function
                } else if previous == b'.' {
                    Highlight::Property
                } else if KEYWORDS.contains(&word) {
                    Highlight::Keyword
                } else if CONSTANTS.contains(&word) {
                    Highlight::Constant
                } else if next == Some(b'(') {
                    Highlight::FunctionCall
                } else if word == "_self" || word == "_context" || word == "loop" {
                    Highlight::SelfVariable
                } else {
                    Highlight::Variable
                };

                self.out.push(token_start..i, kind);
            } else {
                i += 1;
                assigns |= b == b'=' && !matches!(self.bytes.get(i), Some(b'=')) && !matches!(previous, b'=' | b'!' | b'<' | b'>');
                let kind = if b"()[]{},".contains(&b) { Highlight::Punctuation } else { Highlight::Operator };
                self.out.push(token_start..i, kind);
            }

            previous = b;
        }

        if let Some(name) = tag_name {
            let raw = name == "verbatim" || name == "raw";
            let paired = match name.as_str() {
                "block" => token_count <= 2,
                "set" => !assigns,
                _ => true,
            };
            self.out.tags.push(Tag { name: name.clone(), argument, range: start..i, paired });

            if raw && let Some(end) = self.find_tag(i, &format!("end{name}")) {
                return end;
            }
        }

        i
    }

    /// Start of the next `{% name` tag after `from`.
    fn find_tag(&self, from: usize, name: &str) -> Option<usize> {
        let mut at = from;
        while let Some(open) = self.find(at, "{%") {
            let inside = self.text[open + 2..].trim_start_matches(['-', '~']).trim_start();
            if inside.starts_with(name) {
                return Some(open);
            }
            at = open + 2;
        }
        None
    }
}

fn line_of(text: &str, at: usize) -> u32 {
    text[..at].bytes().filter(|b| *b == b'\n').count() as u32
}

/// Structure-view symbols (blocks, macros) and folds (tag pairs, comments).
fn structure(text: &str) -> (Vec<Symbol>, Vec<Fold>) {
    let lexed = lex(text);
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut folds = Vec::new();
    // (tag name, tag start, symbol index)
    let mut open: Vec<(&str, usize, Option<usize>)> = Vec::new();

    for tag in &lexed.tags {
        if let Some(name) = tag.name.strip_prefix("end") {
            let Some(at) = open.iter().rposition(|(n, _, _)| *n == name) else { continue };
            let (_, start, symbol) = open[at];
            open.truncate(at);

            let (start_line, end_line) = (line_of(text, start), line_of(text, tag.range.start));
            if end_line > start_line {
                folds.push(Fold { start_line, end_line, kind: FoldKind::Region });
            }
            if let Some(symbol) = symbol {
                symbols[symbol].range.end = tag.range.end;
            }
            continue;
        }

        let kind = match tag.name.as_str() {
            "block" => Some(SymbolKind::Field),
            "macro" => Some(SymbolKind::Function),
            _ => None,
        };
        let symbol = kind.zip(tag.argument.clone()).map(|(kind, name_range)| {
            let parent = open.iter().rev().find_map(|(_, _, s)| *s);
            symbols.push(Symbol {
                name: text[name_range.clone()].to_owned(),
                detail: None,
                kind,
                range: tag.range.clone(),
                name_range,
                parent,
            });
            symbols.len() - 1
        });
        if tag.paired {
            open.push((tag.name.as_str(), tag.range.start, symbol));
        }
    }

    for comment in &lexed.comments {
        let (start_line, end_line) = (line_of(text, comment.start), line_of(text, comment.end));
        if end_line > start_line {
            folds.push(Fold { start_line, end_line, kind: FoldKind::Comment });
        }
    }

    (symbols, folds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(&str, &'static str)> {
        lex(text).tokens.iter().map(|t| (&text[t.range.clone()], t.kind.name())).collect()
    }

    #[test]
    fn highlights_twig_in_html() {
        let text = r#"<a href="{{ path('home') }}" class="x">{# note #}{{ user.name|upper }}</a>"#;
        assert_eq!(
            kinds(text),
            [
                ("<a", "tag"),
                ("href", "attribute"),
                ("\"", "string"),
                ("{{", "punctuation"),
                ("path", "function-call"),
                ("(", "punctuation"),
                ("'home'", "string"),
                (")", "punctuation"),
                ("}}", "punctuation"),
                ("\"", "string"),
                ("class", "attribute"),
                ("\"x\"", "string"),
                (">", "tag"),
                ("{# note #}", "comment"),
                ("{{", "punctuation"),
                ("user", "variable"),
                (".", "operator"),
                ("name", "property"),
                ("|", "operator"),
                ("upper", "function"),
                ("}}", "punctuation"),
                ("</a", "tag"),
                (">", "tag"),
            ]
        );
    }

    #[test]
    fn highlights_tags() {
        assert_eq!(
            kinds("{%- if items is not empty and true -%}"),
            [
                ("{%-", "punctuation"),
                ("if", "keyword"),
                ("items", "variable"),
                ("is", "keyword"),
                ("not", "keyword"),
                ("empty", "variable"),
                ("and", "keyword"),
                ("true", "constant"),
                ("-%}", "punctuation"),
            ]
        );
    }

    #[test]
    fn skips_verbatim_contents() {
        let text = "{% verbatim %}{{ not twig }}{% endverbatim %}";
        let names: Vec<_> = kinds(text).into_iter().map(|(t, _)| t).collect();
        assert_eq!(names, ["{%", "verbatim", "%}", "{%", "endverbatim", "%}"]);
    }

    #[test]
    fn outlines_and_folds_blocks() {
        let text = "{% extends 'base.html.twig' %}\n{% block body %}\n  {% set x = 1 %}\n  {% block inner %}\n  {% endblock %}\n  {% for i in 1..3 %}\n  {% endfor %}\n  {% block title 'Short' %}\n{% endblock %}\n{#\n long\n#}\n{% macro field(name) %}\n{% endmacro %}\n";
        let (symbols, folds) = structure(text);

        let outline: Vec<_> = symbols.iter().map(|s| (s.name.as_str(), s.kind, s.parent)).collect();
        assert_eq!(
            outline,
            [
                ("body", SymbolKind::Field, None),
                ("inner", SymbolKind::Field, Some(0)),
                ("title", SymbolKind::Field, Some(0)),
                ("field", SymbolKind::Function, None),
            ]
        );
        assert!(text[symbols[0].range.clone()].starts_with("{% block body %}"));
        assert!(text[symbols[0].range.clone()].ends_with("{% endblock %}"));

        let lines: Vec<_> = folds.iter().map(|f| (f.start_line, f.end_line, f.kind)).collect();
        assert_eq!(
            lines,
            [
                (3, 4, FoldKind::Region),
                (5, 6, FoldKind::Region),
                (1, 8, FoldKind::Region),
                (12, 13, FoldKind::Region),
                (9, 11, FoldKind::Comment),
            ]
        );
    }
}
