//! Twig for OpenIDEA: highlighting, structure view (blocks, macros) and
//! folding (tag pairs, comments) from a hand-written lexer, exported as the
//! plugin's `syntax-provider`. Detection comes from `languages/twig.toml`.

mod checks;
mod complete;
mod navigate;

use std::collections::BTreeMap;
use std::ops::Range;

use std::path::Path;

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, Providers};
use openidea_plugin::syntax::{Fold, FoldKind, Highlight, Symbol, SymbolKind, SyntaxProvider, Token};
use openidea_plugin::text::{offset, position};
use openidea_plugin::{
    Context, Diagnostic, Error, Event, EventKind, Invocation, Location, LogLevel, Plugin, Project, Range as EditorRange, Severity, host,
};
use navigate::{Extension, ExtensionKind, Import};

const SOURCE: &str = "Twig";

const EXTENSION_GLOBS: &[&str] = &["src/**/*.php", "app/**/*.php", "lib/**/*.php"];
const EXTENSION_FILES: u32 = 1500;

#[derive(Default)]
pub struct Twig {
    /// Filters, functions and tests registered in the project's PHP, by absolute file path.
    extensions: BTreeMap<String, Vec<Extension>>,
}

impl Twig {
    fn scan_extensions(&mut self) {
        if !host::setting_bool("projectExtensions").unwrap_or(true) {
            return;
        }

        for project in host::projects().unwrap_or_default() {
            for glob in EXTENSION_GLOBS {
                for path in project.find_files(glob, EXTENSION_FILES).unwrap_or_default() {
                    self.scan_file(&project, &path);
                }
            }
        }
    }

    fn scan_file(&mut self, project: &Project, path: &str) {
        let found = project.read_file(path).map(|php| navigate::extensions(&php)).unwrap_or_default();
        if found.is_empty() {
            self.extensions.remove(path);
            return;
        }

        self.extensions.insert(path.to_owned(), found);
    }

    fn extra_names(&self, root: &str, kind: ExtensionKind) -> Vec<&str> {
        let inside = |(path, _): &(&String, &Vec<Extension>)| Path::new(path.as_str()).starts_with(root);
        let mut names: Vec<&str> =
            self.extensions.iter().filter(inside).flat_map(|(_, found)| found).filter(|e| e.kind == kind).map(|e| e.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        names
    }
}

fn template_path(project: &Project, name: &str) -> Option<String> {
    if name.contains("..") || name.starts_with('/') || name.is_empty() {
        return None;
    }

    let root = project.root().ok()?;
    let direct = format!("templates/{name}");
    if project.read_file(&direct).is_ok() {
        return Some(format!("{}/{direct}", root.trim_end_matches('/')));
    }

    let wanted = format!("/{name}");
    let mut found = project.find_files(&format!("**/{name}"), 20).unwrap_or_default();
    found.retain(|path| path.ends_with(&wanted));
    found.sort_by_key(|path| (!path.contains("/templates/"), path.len()));
    found.into_iter().next()
}

fn macros_of(project: &Project, current: &Document, template: &str) -> Option<(String, String)> {
    if template == "_self" {
        return Some((current.path.clone(), current.text.clone()));
    }

    let path = template_path(project, template)?;
    let text = project.read_file(&path).ok()?;
    Some((path, text))
}

fn in_twig_code(text: &str, offset: usize) -> bool {
    let before = &text[..offset];
    let open = ["{{", "{%"].iter().filter_map(|o| before.rfind(o)).max();
    let close = ["}}", "%}", "#}"].iter().filter_map(|c| before.rfind(c)).max();

    open.is_some_and(|o| close.is_none_or(|c| c < o))
}

fn macro_items(project: &Project, doc: &Document, offset: usize) -> Vec<CompletionItem> {
    let (receiver, range) = match navigate::dotted_at(&doc.text, offset) {
        Some((receiver, _, range)) => (receiver, range),
        None => (None, offset..offset),
    };
    let replace = EditorRange::new(position(&doc.text, range.start), position(&doc.text, range.end));

    let template = match receiver {
        Some(alias) if in_twig_code(&doc.text, range.start) => navigate::imports(&doc.text).into_iter().find_map(|import| match import {
            Import::Alias { template, alias: name } if name == alias => Some(template),
            _ => None,
        }),
        Some(_) => None,
        None => from_import_template(&doc.text, range.start),
    };
    let Some(template) = template else { return Vec::new() };
    let Some((_, text)) = macros_of(project, doc, &template) else { return Vec::new() };

    navigate::macros(&text)
        .into_iter()
        .map(|m| {
            let mut item = CompletionItem::new(&m.name, CompletionKind::Function);
            item.detail = Some(format!("macro({})", m.arguments));
            item.replace = Some(replace);
            item
        })
        .collect()
}

/// The template of a `{% from 'x' import …` tag the caret is inside the name list of.
fn from_import_template(text: &str, offset: usize) -> Option<String> {
    let tag_start = text[..offset].rfind("{%")?;
    if text[tag_start..offset].contains("%}") {
        return None;
    }

    let body = text[tag_start + 2..offset].trim_start_matches(['-', '~']).trim_start();
    let rest = body.strip_prefix("from")?.trim_start();
    let quote = rest.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let inner = &rest[1..];
    let end = inner.find(quote)?;
    let after = inner[end + 1..].trim_start();

    after.starts_with("import").then(|| inner[..end].to_owned())
}

impl Plugin for Twig {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        for kind in [EventKind::FileOpened, EventKind::FileSaved] {
            if let Err(error) = host::subscribe(kind) {
                host::log(LogLevel::Warn, &format!("Twig inspections need the {} event: {error}", kind.name()));
            }
        }

        self.scan_extensions();

        Ok(())
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        for event in events {
            let (Event::FileOpened(path) | Event::FileSaved(path)) = event else { continue };
            if path.ends_with(".twig") {
                inspect(path);
            }

            let saved_php = matches!(event, Event::FileSaved(_)) && path.ends_with(".php");
            if let (true, Some(project)) = (saved_php, project_of(path)) {
                self.scan_file(&project, path);
            }
        }
    }

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

impl Providers for Twig {
    fn completion(&mut self, doc: &Document, at: openidea_plugin::Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(offset) = offset(&doc.text, at) else { return Vec::new() };
        let project = project_of(&doc.path);

        let macros = project.as_ref().map(|p| macro_items(p, doc, offset)).unwrap_or_default();
        if !macros.is_empty() {
            return macros;
        }

        let names = !project.as_ref().is_some_and(is_symfony);
        let root = project.as_ref().and_then(|p| p.root().ok()).unwrap_or_default();
        let extra = complete::Extra {
            filters: self.extra_names(&root, ExtensionKind::Filter),
            functions: self.extra_names(&root, ExtensionKind::Function),
            tests: self.extra_names(&root, ExtensionKind::Test),
        };

        complete::items_with(&doc.text, offset, names, &extra)
    }

    fn definition(&mut self, doc: &Document, at: openidea_plugin::Position) -> Vec<Location> {
        let Some(offset) = offset(&doc.text, at) else { return Vec::new() };
        let Some(project) = project_of(&doc.path) else { return Vec::new() };
        let range_in = |text: &str, range: Range<usize>| EditorRange::new(position(text, range.start), position(text, range.end));

        if let Some((name, _)) = navigate::template_at(&doc.text, offset) {
            if is_symfony(&project) {
                return Vec::new();
            }

            return template_path(&project, &name).map(|path| vec![Location { path, range: EditorRange::new(position("", 0), position("", 0)) }]).unwrap_or_default();
        }

        let Some((receiver, word, _)) = navigate::dotted_at(&doc.text, offset) else { return Vec::new() };
        let imports = navigate::imports(&doc.text);
        let template = match &receiver {
            Some(alias) => imports.iter().find_map(|import| match import {
                Import::Alias { template, alias: name } if name == alias => Some((template.clone(), word.clone())),
                _ => None,
            }),
            None => imports.iter().find_map(|import| match import {
                Import::Names { template, names } => names.iter().find(|(_, used)| *used == word).map(|(original, _)| (template.clone(), original.clone())),
                _ => None,
            }),
        };

        if let Some((template, name)) = template {
            let Some((path, text)) = macros_of(&project, doc, &template) else { return Vec::new() };
            return navigate::macros(&text)
                .into_iter()
                .filter(|m| m.name == name)
                .map(|m| Location { path: path.clone(), range: range_in(&text, m.name_range) })
                .collect();
        }

        if receiver.is_some() || is_symfony(&project) {
            return Vec::new();
        }

        let before = doc.text[..offset].trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
        let kind = if before.ends_with('|') {
            ExtensionKind::Filter
        } else if before.trim_end().ends_with(" is") || before.trim_end().ends_with(" not") {
            ExtensionKind::Test
        } else {
            ExtensionKind::Function
        };
        let root = project.root().unwrap_or_default();

        self.extensions
            .iter()
            .filter(|(path, _)| Path::new(path.as_str()).starts_with(&root))
            .flat_map(|(path, found)| found.iter().map(move |e| (path, e)))
            .filter(|(_, e)| e.kind == kind && e.name == word)
            .filter_map(|(path, e)| {
                let php = project.read_file(path).ok()?;
                Some(Location { path: path.clone(), range: range_in(&php, e.range.clone()) })
            })
            .collect()
    }
}

fn project_of(path: &str) -> Option<Project> {
    host::projects()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| Some((p.root().ok()?, p)))
        .filter(|(root, _)| Path::new(path).starts_with(root))
        .max_by_key(|(root, _)| root.len())
        .map(|(_, project)| project)
}

/// The test `aritti.symfony` uses to switch on. Inside such a project that
/// plugin owns tag, filter, function and test completion, so this one stays
/// silent there instead of listing everything twice.
fn is_symfony(project: &Project) -> bool {
    project.read_file("symfony.lock").is_ok()
        || project
            .read_file("composer.json")
            .is_ok_and(|composer| composer.contains("\"symfony/framework-bundle\"") || composer.contains("\"symfony/symfony\""))
}

fn inspect(path: &str) {
    let Some(project) = project_of(path) else { return };
    let Ok(text) = project.read_file(path) else { return };

    let enabled = host::setting_bool("diagnostics").unwrap_or(true);
    let mut findings = checks::check(&text);
    if host::setting_bool("expressionErrors").unwrap_or(false) {
        findings.extend(checks::brackets(&text));
    }

    let diagnostics: Vec<Diagnostic> = findings
        .into_iter()
        .filter(|_| enabled)
        .map(|f| Diagnostic {
            code: Some(f.code.into()),
            source: Some(SOURCE.into()),
            ..Diagnostic::new(
                EditorRange::new(position(&text, f.range.start), position(&text, f.range.end)),
                Severity::Error,
                f.message,
            )
        })
        .collect();

    let _ = host::publish_diagnostics(path, &diagnostics);
}

// `register_plugin!` has no arm for syntax with providers; this is its
// expansion put together.
#[cfg(not(target_arch = "wasm32"))]
const _: fn() -> Twig = <Twig as Default>::default;
#[cfg(target_arch = "wasm32")]
const _: () = {
    use openidea_plugin::syntax_bindings::exports::openidea::plugin::syntax_provider as wit;

    struct Component;
    openidea_plugin::__plugin_exports!(Twig, Component, openidea_plugin::bindings);
    openidea_plugin::__providers_exports!(Twig, Component);

    impl wit::Guest for Component {
        fn highlights(language: String, text: String) -> Vec<wit::Token> {
            openidea_plugin::__rt::highlights::<Twig>(&language, &text)
        }

        fn outline(language: String, text: String) -> Vec<wit::Symbol> {
            openidea_plugin::__rt::outline::<Twig>(&language, &text)
        }

        fn folding(language: String, text: String) -> Vec<wit::Fold> {
            openidea_plugin::__rt::folding::<Twig>(&language, &text)
        }
    }

    openidea_plugin::bindings::__export_plugin_world!(Component with_types_in openidea_plugin::bindings);
    openidea_plugin::syntax_bindings::__export_syntax_world!(Component with_types_in openidea_plugin::syntax_bindings);
};

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
    /// Openers (`{{`, `{%`, `{#`) whose closer is missing.
    unterminated: Vec<Range<usize>>,
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
            let closer = self.find(start + 2, "#}");
            if closer.is_none() {
                self.out.unterminated.push(start..start + 2);
            }
            let end = closer.map_or(self.bytes.len(), |e| e + 2);
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
        let mut closed = false;

        while i < self.bytes.len() {
            let b = self.bytes[i];

            let trimmed_close = (b == b'-' || b == b'~') && self.at(i + 1, close);
            if self.at(i, close) || trimmed_close {
                let end = i + close.len() + usize::from(trimmed_close);
                self.out.push(i..end, Highlight::Punctuation);
                i = end;
                closed = true;
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

        if !closed {
            self.out.unterminated.push(start..start + 2);
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

    fn project(files: &[(&str, &str)]) -> openidea_plugin::mock::MockHost {
        let host = openidea_plugin::mock::MockHost::install();
        host.set_project_root(Some("/p"));
        for (path, text) in files {
            host.add_file(path, text);
        }
        host
    }

    fn open(host: &openidea_plugin::mock::MockHost, path: &str) -> Vec<(Option<String>, Option<String>)> {
        Twig::default().handle_events(&[Event::FileOpened(path.into())], &host.invocation());
        host.diagnostics().get(path).into_iter().flatten().map(|d| (d.code.clone(), d.source.clone())).collect()
    }

    #[test]
    fn publishes_findings_for_a_twig_file() {
        let host = project(&[("/p/a.html.twig", "{% if a %}\n{{ x ")]);
        let found = open(&host, "/p/a.html.twig");
        assert!(found.contains(&(Some("unclosed-tag".into()), Some("Twig".into()))));
        assert!(found.iter().any(|(code, _)| code.as_deref() == Some("unterminated-delimiter")));
    }

    #[test]
    fn diagnostics_can_be_switched_off() {
        let host = project(&[("/p/a.html.twig", "{% if a %}")]);
        host.set_setting("diagnostics", "false");
        assert!(open(&host, "/p/a.html.twig").is_empty());
    }

    #[test]
    fn completes_names_outside_symfony_but_not_inside() {
        let doc = |path: &str| Document { path: path.into(), language: "twig".into(), version: 1, text: "{% i".into() };
        let at = openidea_plugin::Position::new(0, 4);

        let _plain = project(&[("composer.json", "{}")]);
        assert!(Twig::default().completion(&doc("/p/a.twig"), at, None).iter().any(|i| i.label == "endif"));

        let _symfony = project(&[("composer.json", r#"{"require": {"symfony/framework-bundle": "^7"}}"#)]);
        assert!(Twig::default().completion(&doc("/p/a.twig"), at, None).is_empty());
    }
}

#[cfg(test)]
mod project_tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    const FORMS: &str = "{% macro input(name, value = '') %}<input>{% endmacro %}\n{% macro button(label) %}<b>{% endmacro %}\n";

    fn host() -> MockHost {
        let host = MockHost::install();
        host.add_file("composer.json", r#"{"require": {"twig/twig": "^3"}}"#);
        host.add_file("templates/forms.html.twig", FORMS);
        host.add_file("src/Ext.php", "<?php\nclass Ext { function f() { return [new TwigFilter('money', fn() => 1), new TwigFunction('asset_url', fn() => 1)]; } }\n");
        host
    }

    fn doc(text: &str) -> Document {
        Document { path: "/project/templates/page.html.twig".into(), language: "twig".into(), version: 1, text: text.into() }
    }

    fn at(marked: &str) -> (Document, openidea_plugin::Position) {
        let offset = marked.find('^').unwrap();
        let text = marked.replacen('^', "", 1);
        let position = position(&text, offset);
        (doc(&text), position)
    }

    fn labels(items: Vec<CompletionItem>) -> Vec<String> {
        items.into_iter().map(|i| i.label).collect()
    }

    #[test]
    fn completes_macros_of_an_imported_template() {
        let host = host();
        let mut twig = Twig::default();
        twig.activate(&host.context("aritti.twig")).unwrap();

        let (d, p) = at("{% import 'forms.html.twig' as forms %}{{ forms.^ }}");
        assert_eq!(labels(twig.completion(&d, p, None)), ["input", "button"]);
        let (d, p) = at("{% import 'forms.html.twig' as forms %}{{ forms.bu^ }}");
        assert_eq!(labels(twig.completion(&d, p, None)), ["input", "button"]);
        let (d, p) = at("{% from 'forms.html.twig' import ^ %}");
        assert_eq!(labels(twig.completion(&d, p, None)), ["input", "button"]);
        let (d, p) = at("{% macro local(a) %}{% endmacro %}{% import _self as me %}{{ me.^ }}");
        assert_eq!(labels(twig.completion(&d, p, None)), ["local"]);
        let (d, p) = at("{{ other.^ }}");
        assert!(labels(twig.completion(&d, p, None)).is_empty());
    }

    #[test]
    fn goes_to_macros_and_templates() {
        let host = host();
        let mut twig = Twig::default();
        twig.activate(&host.context("aritti.twig")).unwrap();

        let (d, p) = at("{% import 'forms.html.twig' as forms %}{{ forms.but^ton('x') }}");
        let found = twig.definition(&d, p);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].path.as_str(), found[0].range.start.line), ("/project/templates/forms.html.twig", 1));

        let (d, p) = at("{% from 'forms.html.twig' import input as field %}{{ fie^ld('a') }}");
        assert_eq!(twig.definition(&d, p)[0].range.start.line, 0);

        let (d, p) = at("{% extends 'forms.ht^ml.twig' %}");
        assert_eq!(twig.definition(&d, p)[0].path, "/project/templates/forms.html.twig");
    }

    #[test]
    fn project_extension_names_complete_and_navigate_outside_symfony() {
        let host = host();
        let mut twig = Twig::default();
        twig.activate(&host.context("aritti.twig")).unwrap();

        let (d, p) = at("{{ price|mo^ }}");
        assert!(labels(twig.completion(&d, p, None)).contains(&"money".to_owned()));
        let (d, p) = at("{{ asse^ }}");
        assert!(labels(twig.completion(&d, p, None)).contains(&"asset_url".to_owned()));
        let (d, p) = at("{{ price|mon^ey }}");
        let found = twig.definition(&d, p);
        assert_eq!((found[0].path.as_str(), found[0].range.start.line), ("/project/src/Ext.php", 1));

        host.add_file("symfony.lock", "{}");
        let (d, p) = at("{{ price|mo^ }}");
        assert!(twig.completion(&d, p, None).is_empty());
        let (d, p) = at("{{ price|mon^ey }}");
        assert!(twig.definition(&d, p).is_empty());
    }
}
