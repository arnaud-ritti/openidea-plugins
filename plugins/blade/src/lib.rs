//! Blade for OpenIDEA: highlighting, structure view (sections, pushes) and
//! folding (directive pairs, comments) from a hand-written lexer, exported as
//! the plugin's `syntax-provider`. Detection comes from `languages/blade.toml`.

use std::ops::Range;

use openidea_plugin::syntax::{Fold, FoldKind, Highlight, Symbol, SymbolKind, SyntaxProvider, Token};
use openidea_plugin::{Error, Invocation, Plugin, register_plugin};

#[derive(Default)]
pub struct Blade;

impl Plugin for Blade {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

impl SyntaxProvider for Blade {
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

register_plugin!(Blade, syntax);

const PHP_KEYWORDS: &[&str] = &[
    "as", "if", "else", "elseif", "for", "foreach", "while", "return", "new", "instanceof", "fn", "function", "use", "and",
    "or", "xor", "echo", "match", "static", "isset", "empty", "list", "array", "clone", "print", "throw", "try", "catch",
    "finally",
];
const PHP_CONSTANTS: &[&str] = &["true", "false", "null", "TRUE", "FALSE", "NULL"];

/// Directives that open a block, and the ones that close it.
const BLOCKS: &[(&str, &[&str])] = &[
    ("if", &["endif"]),
    ("hasSection", &["endif"]),
    ("sectionMissing", &["endif"]),
    ("unless", &["endunless"]),
    ("isset", &["endisset"]),
    ("empty", &["endempty"]),
    ("auth", &["endauth"]),
    ("guest", &["endguest"]),
    ("production", &["endproduction"]),
    ("env", &["endenv"]),
    ("section", &["endsection", "show", "stop", "append", "overwrite"]),
    ("push", &["endpush"]),
    ("pushIf", &["endPushIf"]),
    ("prepend", &["endprepend"]),
    ("pushOnce", &["endPushOnce"]),
    ("prependOnce", &["endPrependOnce"]),
    ("once", &["endonce"]),
    ("for", &["endfor"]),
    ("foreach", &["endforeach"]),
    ("forelse", &["endforelse"]),
    ("while", &["endwhile"]),
    ("switch", &["endswitch"]),
    ("php", &["endphp"]),
    ("verbatim", &["endverbatim"]),
    ("error", &["enderror"]),
    ("component", &["endcomponent"]),
    ("slot", &["endslot"]),
    ("can", &["endcan"]),
    ("cannot", &["endcannot"]),
    ("canany", &["endcanany"]),
    ("session", &["endsession"]),
    ("fragment", &["endfragment"]),
    ("persist", &["endpersist"]),
    ("teleport", &["endteleport"]),
    ("script", &["endscript"]),
    ("assets", &["endassets"]),
];

/// Directives that neither open nor close a block.
// ponytail: only Laravel's own directives (and Livewire's) are known; custom ones from Blade::directive() stay plain text.
const STANDALONE: &[&str] = &[
    "else", "elseif", "elsecan", "elsecannot", "elsecanany", "extends", "yield", "parent", "include", "includeIf",
    "includeWhen", "includeUnless", "includeFirst", "each", "stack", "continue", "break", "case", "default", "props",
    "aware", "class", "style", "checked", "selected", "disabled", "readonly", "required", "csrf", "method", "json", "js",
    "lang", "choice", "use", "inject", "vite", "viteReactRefresh", "dd", "dump", "livewire", "livewireStyles",
    "livewireScripts", "livewireScriptConfig", "bool", "unlessenv",
];

fn is_directive(name: &str) -> bool {
    STANDALONE.contains(&name) || BLOCKS.iter().any(|(open, closers)| *open == name || closers.contains(&name))
}

/// The closers of a directive that opens a block, if this use of it does.
fn opens(name: &str, has_arguments: bool, argument_count: usize) -> Option<&'static [&'static str]> {
    let (_, closers) = BLOCKS.iter().find(|(open, _)| *open == name)?;
    let inline = match name {
        // `@section('title', 'Home')` is complete on its own.
        "section" => argument_count >= 2,
        "php" => has_arguments,
        // Without arguments `@empty` is the separator of `@forelse`.
        "empty" => !has_arguments,
        _ => false,
    };
    (!inline).then_some(closers)
}

/// Directives whose first string argument names something worth listing.
fn symbol_kind(name: &str) -> Option<SymbolKind> {
    matches!(name, "section" | "push" | "pushOnce" | "prepend" | "prependOnce" | "fragment").then_some(SymbolKind::Field)
}

#[derive(Debug, PartialEq, Eq)]
struct Directive {
    name: String,
    /// Between the parentheses.
    arguments: Option<Range<usize>>,
    range: Range<usize>,
}

#[derive(Default)]
struct Lexed {
    tokens: Vec<Token>,
    directives: Vec<Directive>,
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
    lexer.markup();
    lexer.out
}

impl Lexer<'_> {
    fn at(&self, i: usize, s: &str) -> bool {
        self.bytes[i..].starts_with(s.as_bytes())
    }

    fn find(&self, from: usize, s: &str) -> Option<usize> {
        self.text[from..].find(s).map(|i| from + i)
    }

    fn line_end(&self, from: usize) -> usize {
        self.text[from..].find('\n').map_or(self.bytes.len(), |i| from + i)
    }

    fn markup(&mut self) {
        let mut i = 0;
        while i < self.bytes.len() {
            if let Some(end) = self.construct(i) {
                i = end;
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

    /// `<name attr="value" …>`, with Blade anywhere in it.
    fn element(&mut self, start: usize) -> usize {
        let mut i = start + 1;
        while i < self.bytes.len() && (is_identifier(self.bytes[i]) || matches!(self.bytes[i], b'/' | b'!' | b'-' | b':' | b'.')) {
            i += 1;
        }
        self.out.push(start..i, Highlight::Tag);

        while i < self.bytes.len() {
            let b = self.bytes[i];

            if let Some(end) = self.construct(i) {
                i = end;
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

            let first_token = self.out.tokens.len();
            if let Some(end) = self.construct(i) {
                if segment < i {
                    self.out.tokens.insert(first_token, Token { range: segment..i, kind: Highlight::String });
                }
                i = end;
                segment = i;
                continue;
            }

            i += 1;
        }

        self.out.push(segment..i, Highlight::String);
        i
    }

    /// A Blade comment, echo, directive or PHP block starting at `i`; returns its end.
    fn construct(&mut self, i: usize) -> Option<usize> {
        match self.bytes[i] {
            b'{' if self.at(i, "{{--") => {
                let end = self.find(i + 4, "--}}").map_or(self.bytes.len(), |e| e + 4);
                self.out.push(i..end, Highlight::Comment);
                self.out.comments.push(i..end);
                Some(end)
            }
            b'{' if self.at(i, "{{") => Some(self.echo(i, 2, "}}")),
            b'{' if self.at(i, "{!!") => Some(self.echo(i, 3, "!!}")),
            // `@@if` and `@{{ x }}` print as written.
            b'@' if self.at(i, "@@") => Some(i + 2),
            b'@' if self.at(i, "@{{") => Some(self.find(i, "}}").map_or(self.bytes.len(), |e| e + 2)),
            b'@' => self.directive(i),
            b'<' if self.at(i, "<?") => Some(self.php_block(i)),
            _ => None,
        }
    }

    fn echo(&mut self, start: usize, open_length: usize, close: &str) -> usize {
        let body = start + open_length;
        self.out.push(start..body, Highlight::Punctuation);

        let Some(end) = self.find(body, close) else {
            self.php(body, self.bytes.len());
            return self.bytes.len();
        };

        self.php(body, end);
        self.out.push(end..end + close.len(), Highlight::Punctuation);
        end + close.len()
    }

    fn php_block(&mut self, start: usize) -> usize {
        let open_length = ["<?php", "<?="].iter().find(|open| self.at(start, open)).map_or(2, |open| open.len());
        let body = start + open_length;
        self.out.push(start..body, Highlight::Punctuation);

        let Some(end) = self.find(body, "?>") else {
            self.php(body, self.bytes.len());
            return self.bytes.len();
        };

        self.php(body, end);
        self.out.push(end..end + 2, Highlight::Punctuation);
        end + 2
    }

    fn directive(&mut self, at: usize) -> Option<usize> {
        // An email address, not a directive.
        if at > 0 && is_identifier(self.bytes[at - 1]) {
            return None;
        }

        let name_start = at + 1;
        let mut name_end = name_start;
        while name_end < self.bytes.len() && self.bytes[name_end].is_ascii_alphabetic() {
            name_end += 1;
        }

        let name = &self.text[name_start..name_end];
        let continues = self.bytes.get(name_end).is_some_and(|b| is_identifier(*b));
        if continues || !is_directive(name) {
            return None;
        }

        self.out.push(at..name_end, Highlight::Keyword);

        let mut open = name_end;
        while open < self.bytes.len() && matches!(self.bytes[open], b' ' | b'\t') {
            open += 1;
        }

        let mut arguments = None;
        let mut end = name_end;
        if self.bytes.get(open) == Some(&b'(') {
            let close = self.closing_parenthesis(open);
            let body_end = close.unwrap_or_else(|| self.line_end(open));

            self.out.push(open..open + 1, Highlight::Punctuation);
            self.php(open + 1, body_end);
            if close.is_some() {
                self.out.push(body_end..body_end + 1, Highlight::Punctuation);
            }

            arguments = Some(open + 1..body_end);
            end = close.map_or(body_end, |c| c + 1);
        }

        let has_arguments = arguments.is_some();
        self.out.directives.push(Directive { name: name.to_owned(), arguments, range: at..end });

        if name == "verbatim" {
            return Some(self.find_directive(end, "endverbatim").unwrap_or(self.bytes.len()));
        }

        if name == "php" && !has_arguments && let Some(close) = self.find_directive(end, "endphp") {
            self.php(end, close);
            return Some(close);
        }

        Some(end)
    }

    /// Start of the next `@name` directive after `from`.
    fn find_directive(&self, from: usize, name: &str) -> Option<usize> {
        let needle = format!("@{name}");
        let mut at = from;
        while let Some(found) = self.find(at, &needle) {
            let after = found + needle.len();
            if !self.bytes.get(after).is_some_and(|b| is_identifier(*b)) {
                return Some(found);
            }
            at = after;
        }
        None
    }

    /// The `)` closing the `(` at `open`, skipping strings.
    fn closing_parenthesis(&self, open: usize) -> Option<usize> {
        let mut depth = 0;
        let mut i = open;

        while i < self.bytes.len() {
            match self.bytes[i] {
                b'\'' | b'"' => i = self.string_end(i) - 1,
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
            i += 1;
        }

        None
    }

    /// The index after the string literal whose opening quote is at `start`.
    fn string_end(&self, start: usize) -> usize {
        let quote = self.bytes[start];
        let mut i = start + 1;

        while i < self.bytes.len() {
            if self.bytes[i] == quote {
                return i + 1;
            }
            i += if self.bytes[i] == b'\\' { 2 } else { 1 };
        }

        self.bytes.len()
    }

    /// PHP between `start` and `end`: strings, variables, numbers, calls and keywords.
    fn php(&mut self, start: usize, end: usize) {
        let mut i = start;

        while i < end {
            let b = self.bytes[i];
            let token_start = i;

            if b.is_ascii_whitespace() {
                i += 1;
            } else if b == b'"' || b == b'\'' {
                i = self.string_end(i).min(end);
                self.out.push(token_start..i, Highlight::String);
            } else if b == b'$' {
                i += 1;
                while i < end && is_identifier(self.bytes[i]) {
                    i += 1;
                }
                let kind = if &self.text[token_start..i] == "$this" { Highlight::SelfVariable } else { Highlight::Variable };
                self.out.push(token_start..i, kind);
            } else if b.is_ascii_digit() {
                while i < end && (self.bytes[i].is_ascii_digit() || self.bytes[i] == b'.' || self.bytes[i] == b'_') {
                    i += 1;
                }
                self.out.push(token_start..i, Highlight::Number);
            } else if is_identifier_start(b) || b == b'\\' {
                while i < end && (is_identifier(self.bytes[i]) || self.bytes[i] == b'\\') {
                    i += 1;
                }
                let kind = self.php_word_kind(start, token_start, i, end);
                if let Some(kind) = kind {
                    self.out.push(token_start..i, kind);
                }
            } else if b.is_ascii() {
                i += 1;
                let kind = if b"()[]{},;".contains(&b) { Highlight::Punctuation } else { Highlight::Operator };
                self.out.push(token_start..i, kind);
            } else {
                i += self.text[i..].chars().next().map_or(1, char::len_utf8);
            }
        }
    }

    fn php_word_kind(&self, region_start: usize, word_start: usize, word_end: usize, region_end: usize) -> Option<Highlight> {
        let word = &self.text[word_start..word_end];
        let before = self.text[region_start..word_start].trim_end();
        let after = self.text[word_end..region_end].trim_start();
        let is_call = after.starts_with('(');

        if before.ends_with("->") || before.ends_with("?->") {
            return Some(if is_call { Highlight::FunctionCall } else { Highlight::Property });
        }

        if before.ends_with("::") {
            return Some(if is_call { Highlight::FunctionCall } else { Highlight::Constant });
        }

        if PHP_KEYWORDS.contains(&word) {
            return Some(Highlight::Keyword);
        }

        if PHP_CONSTANTS.contains(&word) {
            return Some(Highlight::Constant);
        }

        if is_call {
            return Some(Highlight::FunctionCall);
        }

        let is_class = word.trim_start_matches('\\').bytes().next().is_some_and(|b| b.is_ascii_uppercase());
        is_class.then_some(Highlight::Type)
    }
}

fn line_of(text: &str, at: usize) -> u32 {
    text[..at].bytes().filter(|b| *b == b'\n').count() as u32
}

/// The contents of the first string literal of `arguments`.
fn first_string(text: &str, arguments: &Range<usize>) -> Option<Range<usize>> {
    let inside = &text[arguments.clone()];
    let open = inside.find(['\'', '"'])?;
    let quote = inside.as_bytes()[open];
    let length = inside[open + 1..].bytes().position(|b| b == quote)?;
    let start = arguments.start + open + 1;
    Some(start..start + length)
}

/// Commas outside strings and brackets, plus one.
fn argument_count(text: &str, arguments: &Range<usize>) -> usize {
    let bytes = text.as_bytes();
    let mut depth = 0;
    let mut quote: Option<u8> = None;
    let mut count = 1;
    let mut i = arguments.start;

    while i < arguments.end {
        let b = bytes[i];
        match quote {
            Some(_) if b == b'\\' => i += 1,
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'\'' || b == b'"' => quote = Some(b),
            None if b"([{".contains(&b) => depth += 1,
            None if b")]}".contains(&b) => depth -= 1,
            None if b == b',' && depth == 0 => count += 1,
            None => {}
        }
        i += 1;
    }

    count
}

struct Open {
    closers: &'static [&'static str],
    start: usize,
    symbol: Option<usize>,
}

/// Structure-view symbols (sections, pushes) and folds (directive pairs, comments).
fn structure(text: &str) -> (Vec<Symbol>, Vec<Fold>) {
    let lexed = lex(text);
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut folds = Vec::new();
    let mut open: Vec<Open> = Vec::new();

    for directive in &lexed.directives {
        let name = directive.name.as_str();

        if let Some(at) = open.iter().rposition(|o| o.closers.contains(&name)) {
            let opened = open.remove(at);
            open.truncate(at);

            let (start_line, end_line) = (line_of(text, opened.start), line_of(text, directive.range.start));
            if end_line > start_line {
                folds.push(Fold { start_line, end_line, kind: FoldKind::Region });
            }
            if let Some(symbol) = opened.symbol {
                symbols[symbol].range.end = directive.range.end;
            }
            continue;
        }

        let has_arguments = directive.arguments.is_some();
        let count = directive.arguments.as_ref().map_or(0, |a| argument_count(text, a));
        let symbol = symbol_kind(name).zip(directive.arguments.as_ref().and_then(|a| first_string(text, a))).map(|(kind, name_range)| {
            let parent = open.iter().rev().find_map(|o| o.symbol);
            symbols.push(Symbol {
                name: text[name_range.clone()].to_owned(),
                detail: Some(format!("@{name}")),
                kind,
                range: directive.range.clone(),
                name_range,
                parent,
            });
            symbols.len() - 1
        });

        if let Some(closers) = opens(name, has_arguments, count) {
            open.push(Open { closers, start: directive.range.start, symbol });
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
    fn highlights_echoes_in_html() {
        let text = r#"<a href="{{ route('home') }}" class="x">{{-- note --}}{!! $user->name !!}</a>"#;
        assert_eq!(
            kinds(text),
            [
                ("<a", "tag"),
                ("href", "attribute"),
                ("\"", "string"),
                ("{{", "punctuation"),
                ("route", "function-call"),
                ("(", "punctuation"),
                ("'home'", "string"),
                (")", "punctuation"),
                ("}}", "punctuation"),
                ("\"", "string"),
                ("class", "attribute"),
                ("\"x\"", "string"),
                (">", "tag"),
                ("{{-- note --}}", "comment"),
                ("{!!", "punctuation"),
                ("$user", "variable"),
                ("-", "operator"),
                (">", "operator"),
                ("name", "property"),
                ("!!}", "punctuation"),
                ("</a", "tag"),
                (">", "tag"),
            ]
        );
    }

    #[test]
    fn highlights_directives_and_their_arguments() {
        assert_eq!(
            kinds("@foreach ($posts as $post)\n@include('partials.post', ['post' => $post])"),
            [
                ("@foreach", "keyword"),
                ("(", "punctuation"),
                ("$posts", "variable"),
                ("as", "keyword"),
                ("$post", "variable"),
                (")", "punctuation"),
                ("@include", "keyword"),
                ("(", "punctuation"),
                ("'partials.post'", "string"),
                (",", "punctuation"),
                ("[", "punctuation"),
                ("'post'", "string"),
                ("=", "operator"),
                (">", "operator"),
                ("$post", "variable"),
                ("]", "punctuation"),
                (")", "punctuation"),
            ]
        );
    }

    #[test]
    fn leaves_what_blade_leaves_alone() {
        assert!(kinds("mail me at john@if.com").is_empty());
        assert!(kinds("@media (min-width: 1px) { a { color: red } }").is_empty());
        assert!(kinds("@@if(x) @{{ raw }}").is_empty());
        assert!(kinds("<button @click=\"open = !open\">").contains(&("@click", "attribute")));
    }

    #[test]
    fn reads_php_blocks_and_skips_verbatim() {
        let names = |text| kinds(text).into_iter().map(|(t, _)| t).collect::<Vec<_>>();
        assert_eq!(names("@verbatim{{ not blade }}@endverbatim"), ["@verbatim", "@endverbatim"]);
        assert_eq!(names("@php $x = 1; @endphp"), ["@php", "$x", "=", "1", ";", "@endphp"]);
        assert_eq!(names("@php($x = 1)"), ["@php", "(", "$x", "=", "1", ")"]);
    }

    #[test]
    fn outlines_and_folds_directives() {
        let text = "@extends('layouts.app')\n@section('content')\n  @if ($a)\n    x\n  @endif\n  @push('scripts')\n  @endpush\n@endsection\n@section('title', 'Home')\n{{--\n long\n--}}\n@forelse ($xs as $x)\n  y\n@empty\n  z\n@endforelse\n";
        let (symbols, folds) = structure(text);

        let outline: Vec<_> = symbols.iter().map(|s| (s.name.as_str(), s.kind, s.parent)).collect();
        assert_eq!(
            outline,
            [
                ("content", SymbolKind::Field, None),
                ("scripts", SymbolKind::Field, Some(0)),
                ("title", SymbolKind::Field, None)
            ]
        );
        assert!(text[symbols[0].range.clone()].starts_with("@section('content')"));
        assert!(text[symbols[0].range.clone()].ends_with("@endsection"));
        assert!(text[symbols[2].range.clone()].ends_with("'Home')"));

        let lines: Vec<_> = folds.iter().map(|f| (f.start_line, f.end_line, f.kind)).collect();
        assert_eq!(
            lines,
            [
                (2, 4, FoldKind::Region),
                (5, 6, FoldKind::Region),
                (1, 7, FoldKind::Region),
                (12, 16, FoldKind::Region),
                (9, 11, FoldKind::Comment),
            ]
        );
    }

    #[test]
    fn survives_every_prefix() {
        let text = "<div class=\"{{ $a ? 'é' : \"b\" }}\" @if($x)\n@section('é', {{ 'ü' }})\n@php $é = 1;\n<?php echo \"x\" ?>\n{{-- é --}}@verbatim ü @endverbatim\n@foreach ($ys as $y) {!! $y->é !!}";
        for (end, _) in text.char_indices().chain([(text.len(), ' ')]) {
            let prefix = &text[..end];
            let lexed = lex(prefix);
            for token in &lexed.tokens {
                assert!(prefix.is_char_boundary(token.range.start) && prefix.is_char_boundary(token.range.end), "{prefix:?}");
            }
            structure(prefix);
        }
    }
}
