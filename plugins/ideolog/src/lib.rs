//! Ideolog for OpenIDEA: log highlighting, folding of multi-line events and an
//! outline of errors and warnings (the `syntax-provider` of the `ideolog`
//! language), plus navigation commands.
//!
//! Ported from JetBrains' Ideolog (MIT, https://github.com/JetBrains/ideolog,
//! tag pycharm/263.7307.101).

mod log;
mod source;

use log::{Config, Event, Format, Parsed, Severity};
use openidea_plugin::syntax::{Fold, FoldKind, Highlight, Symbol, SymbolKind, SyntaxProvider, Token};
use openidea_plugin::providers::{Document, Providers};
use openidea_plugin::{
    Error, Invocation, Location, LogLevel, NotificationLevel, Plugin, Position, Project, TextEditor, host, text,
};
use std::ops::Range;
use std::rc::Rc;

const CUSTOM_PATTERNS: &str = "customPatterns";
// ponytail: only the head of a big log is parsed, so the provider answers within
// the IDE's 50 ms edit budget; parse incrementally if big logs need colour past it.
const MAX_DOCUMENT_BYTES: usize = 512 << 10;

#[derive(Default)]
pub struct Ideolog {
    /// The `customPatterns` setting it was built from, and the config.
    config: Option<(String, Rc<Config>)>,
}

impl Ideolog {
    fn config(&mut self) -> Rc<Config> {
        let custom = host::setting_string(CUSTOM_PATTERNS).unwrap_or_default();
        if let Some((built_from, config)) = &self.config
            && *built_from == custom
        {
            return config.clone();
        }

        let config = Rc::new(Config::with_custom(&custom));
        for error in &config.errors {
            host::log(LogLevel::Warn, &format!("Ideolog: custom pattern skipped: {error}"));
        }
        self.config = Some((custom, config.clone()));
        config
    }
}

impl Plugin for Ideolog {
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        let editor = ctx.editor().ok_or("No file is open.")?;
        if !matches!(editor.language().as_str(), "ideolog" | "log") {
            return Err("Not a log file.".into());
        }

        let config = self.config();
        match id {
            "jump-to-source" => jump_to_source(&config, ctx, &editor),
            "next-error" => go_to(&config, &editor, "No error below the caret.", |events, at| {
                events.iter().skip(at + 1).find(|(_, severity)| *severity == Some(Severity::Error))
            }),
            "next-entry" => go_to(&config, &editor, "Already at the last log entry.", |events, at| events.get(at + 1)),
            "previous-entry" => go_to(&config, &editor, "Already at the first log entry.", |events, at| at.checked_sub(1).map(|i| &events[i])),
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }
}

impl SyntaxProvider for Ideolog {
    fn highlights(&mut self, _language: &str, text: &str) -> Vec<Token> {
        highlights(&self.config(), head(text))
    }

    fn outline(&mut self, _language: &str, text: &str) -> Vec<Symbol> {
        outline(&self.config(), head(text))
    }

    fn folding(&mut self, _language: &str, text: &str) -> Vec<Fold> {
        folding(&self.config(), head(text))
    }
}

impl Providers for Ideolog {
    /// Cmd+click on a stack frame opens its file and line.
    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let Some(offset) = text::offset(&doc.text, at) else { return Vec::new() };
        let line_start = doc.text[..offset].rfind('\n').map_or(0, |i| i + 1);
        let line = log::lines(&doc.text[line_start..]).next().map_or("", |(_, l)| l);
        let column = offset - line_start;
        let Some(frame) = source::frames(line).into_iter().find(|f| f.span.contains(&column)) else {
            return Vec::new();
        };
        let Some(project) = project_of(&doc.path) else { return Vec::new() };
        let Ok(Some(path)) = source::resolve(&project, &frame) else { return Vec::new() };

        let target = Position::new(frame.line.saturating_sub(1), 0);
        vec![Location { path, range: openidea_plugin::Range { start: target, end: target } }]
    }
}

/// The open project holding `path` (providers get no window context).
fn project_of(path: &str) -> Option<Project> {
    let projects = host::projects().ok()?;
    let root_of = |p: &Project| p.root().ok().filter(|root| path.starts_with(root.as_str()));
    let index = projects.iter().position(|p| root_of(p).is_some()).unwrap_or(0);
    projects.into_iter().nth(index)
}

// ponytail: SDK 0.1.37's `register_plugin!` has no `syntax, providers` arm. This is
// its `syntax` arm plus its (doc(hidden)) `__providers_exports!`; replace it with
// `register_plugin!(Ideolog, syntax, providers)` once the SDK has that arm.
#[cfg(not(target_arch = "wasm32"))]
const _: fn() -> Ideolog = <Ideolog as ::core::default::Default>::default;
#[cfg(target_arch = "wasm32")]
const _: () = {
    use openidea_plugin::syntax_bindings::exports::openidea::plugin::syntax_provider as wit;

    struct __OpenideaComponent;
    openidea_plugin::__plugin_exports!(Ideolog, __OpenideaComponent, openidea_plugin::bindings);
    openidea_plugin::__providers_exports!(Ideolog, __OpenideaComponent);

    impl wit::Guest for __OpenideaComponent {
        fn highlights(language: String, text: String) -> Vec<wit::Token> {
            openidea_plugin::__rt::highlights::<Ideolog>(&language, &text)
        }

        fn outline(language: String, text: String) -> Vec<wit::Symbol> {
            openidea_plugin::__rt::outline::<Ideolog>(&language, &text)
        }

        fn folding(language: String, text: String) -> Vec<wit::Fold> {
            openidea_plugin::__rt::folding::<Ideolog>(&language, &text)
        }
    }

    openidea_plugin::bindings::__export_plugin_world!(__OpenideaComponent with_types_in openidea_plugin::bindings);
    openidea_plugin::syntax_bindings::__export_syntax_world!(__OpenideaComponent with_types_in openidea_plugin::syntax_bindings);
};

/// The document up to the last line break within [`MAX_DOCUMENT_BYTES`].
fn head(text: &str) -> &str {
    if text.len() <= MAX_DOCUMENT_BYTES {
        return text;
    }

    let cut = log::floor_char_boundary(text, MAX_DOCUMENT_BYTES);
    text[..cut].rfind('\n').map_or(&text[..cut], |end| &text[..=end])
}

fn parsed_events<'a>(config: &'a Config, text: &'a str) -> (Option<&'a Format>, Vec<(Event<'a>, Parsed<'a>)>) {
    let format = config.detect(text);
    let events = log::events(format, text).into_iter().map(|e| {
        let parsed = Parsed::new(format, e.text);
        (e, parsed)
    });
    (format, events.collect())
}

fn highlights(config: &Config, text: &str) -> Vec<Token> {
    let (format, events) = parsed_events(config, text);
    let mut out = Vec::new();
    for (event, parsed) in &events {
        for (range, kind) in config.highlight(format, parsed) {
            push_by_line(&mut out, event.text, event.offset, range, kind);
        }
    }
    out
}

/// One token per line: a multi-line event (a stack trace) is coloured on
/// every line, but never across a line break.
fn push_by_line(out: &mut Vec<Token>, event: &str, offset: usize, range: Range<usize>, kind: Highlight) {
    let mut start = range.start;
    for piece in event[range.clone()].split_inclusive('\n') {
        let end = start + piece.trim_end_matches(['\n', '\r']).len();
        if end > start {
            out.push(Token { range: offset + start..offset + end, kind });
        }
        start += piece.len();
    }
}

fn folding(config: &Config, text: &str) -> Vec<Fold> {
    log::events(config.detect(text), text)
        .into_iter()
        .filter(|e| e.lines.len() > 1)
        .map(|e| Fold { start_line: e.lines.start, end_line: e.lines.end - 1, kind: FoldKind::Region })
        .collect()
}

/// Errors and warnings, to jump between them from the structure view.
fn outline(config: &Config, text: &str) -> Vec<Symbol> {
    let (format, events) = parsed_events(config, text);
    let mut symbols = Vec::new();
    for (event, parsed) in &events {
        let Some(severity) = parsed.severity(format) else { continue };

        let first_line = event.text.lines().next().unwrap_or_default().trim_end();
        let name = Some(parsed.message()).filter(|m| !m.is_empty()).unwrap_or(first_line);
        let name_start = event.offset + (name.as_ptr() as usize - event.text.as_ptr() as usize);
        let label = if severity == Severity::Error { "ERROR" } else { "WARN" };
        let detail = match parsed.time(format) {
            Some(time) => format!("{label} {time}"),
            None => label.to_owned(),
        };
        symbols.push(Symbol {
            name: name.to_owned(),
            detail: Some(detail),
            kind: SymbolKind::Event,
            range: event.offset..event.offset + event.text.len(),
            name_range: name_start..name_start + name.len(),
            parent: None,
        });
    }
    symbols
}

/// The caret line (0-based) and byte offset in that line.
fn caret(editor: &TextEditor, text: &str) -> Result<(u32, usize), Error> {
    let selection = editor.selections()?.first().copied().unwrap_or_default();
    let at = text::offset(text, selection.start).unwrap_or(0);
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    Ok((selection.start.line, at - line_start))
}

type Entries = [(Range<u32>, Option<Severity>)];

/// Moves the caret to the start of the event `pick` chooses, given the
/// events and the index of the one at the caret.
fn go_to(
    config: &Config,
    editor: &TextEditor,
    none: &str,
    pick: impl FnOnce(&Entries, usize) -> Option<&(Range<u32>, Option<Severity>)>,
) -> Result<Option<String>, Error> {
    let text = editor.document()?.text;
    let (line, _) = caret(editor, &text)?;
    let (format, events) = parsed_events(config, &text);
    let entries: Vec<_> = events.iter().map(|(e, p)| (e.lines.clone(), p.severity(format))).collect();
    let at = entries.iter().position(|(lines, _)| lines.contains(&line)).unwrap_or(entries.len().saturating_sub(1));

    let Some((lines, _)) = pick(&entries, at) else {
        host::notify(NotificationLevel::Info, "Ideolog", none);
        return Ok(None);
    };
    host::open_location(&editor.path(), lines.start, 0)?;
    Ok(None)
}

/// A stack frame on the caret line (the one under the caret first), else the
/// source line that logged the event.
fn jump_to_source(config: &Config, ctx: &Invocation, editor: &TextEditor) -> Result<Option<String>, Error> {
    let project = ctx.project().ok_or("No project is open.")?;
    let text = editor.document()?.text;
    let (line, column) = caret(editor, &text)?;
    let line_text = log::lines(&text).nth(line as usize).map(|(_, l)| l).unwrap_or_default();

    let mut frames = source::frames(line_text);
    frames.sort_by_key(|f| !f.span.contains(&column));
    for frame in &frames {
        if let Some(path) = source::resolve(&project, frame)? {
            host::open_location(&path, frame.line.saturating_sub(1), 0)?;
            return Ok(None);
        }
    }

    let (format, events) = parsed_events(config, &text);
    let Some((event, parsed)) = events.iter().find(|(e, _)| e.lines.contains(&line)) else { return Ok(None) };
    let message = parsed.message();
    if message.is_empty() {
        return Err("This event has no message to search for.".into());
    }

    match source::find_logging_line(&project, message, parsed.category(format), event.text)? {
        Some((path, line)) => host::open_location(&path, line, 0)?,
        None => host::notify(NotificationLevel::Info, "Ideolog", "No source line found for this event. Open the project that produced the log."),
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    const IDEA: &str = "\
2024-05-01 10:00:00,123 [   1234]   INFO - #c.i.Startup - IDE started
2024-05-01 10:00:01,000 [   1300]   WARN - #c.i.Index - slow indexing
2024-05-01 10:00:02,000 [   1400]  ERROR - #c.i.Boom - card declined for user 42
java.lang.IllegalStateException: boom
\tat com.example.Foo.bar(Foo.java:42)
2024-05-01 10:00:03,000 [   1500]  DEBUG - #c.i.Quiet - nothing to see
";

    fn kinds_on_line(text: &str, line: usize) -> Vec<(String, Highlight)> {
        let (start, content) = log::lines(text).nth(line).unwrap();
        let end = start + content.len();
        highlights(&Config::default(), text)
            .into_iter()
            .filter(|t| t.range.start >= start && t.range.end <= end)
            .map(|t| (text[t.range].to_owned(), t.kind))
            .collect()
    }

    fn open_log(host: &MockHost, line: u32, column: u32) {
        host.set_project_root(Some("/p"));
        host.open_editor("/p/logs/idea.log", "ideolog", IDEA, &[(line, column, line, column)]);
    }

    #[test]
    fn every_default_format_is_detected() {
        let samples = [
            "10:00:00.000|INFO|main|started\n",
            "2024-05-01 10:00:00,123 [   1234]   INFO - #c.i.Startup - IDE started\n",
            "[10:00:00]W: [Step 1/2] something odd\n",
            "05-01 10:00:00.000  1234  5678 E MyTag: failed\n",
            "2024-05-01 10:00:00.000 | ERROR    | app.main:run:12 - failed\n",
        ];
        let config = Config::default();
        for sample in samples {
            assert!(config.detect(sample).is_some(), "{sample}");
        }
        assert!(config.detect("just some text\n").is_none());
    }

    fn classify(line: &str) -> (Option<Severity>, Option<String>) {
        let text = format!("{line}\n");
        let config = Config::default();
        let format = config.detect(&text);
        assert!(format.is_some(), "no format detected for {line}");
        let parsed = Parsed::new(format, line);
        (parsed.severity(format), parsed.time(format).map(str::to_owned))
    }

    #[test]
    fn more_default_formats_are_classified_by_level_and_time() {
        let error = Some(Severity::Error);
        let warning = Some(Severity::Warning);
        let samples = [
            ("2026-11-06T12:00:00Z ERROR msg", error, Some("2026-11-06T12:00:00Z")),
            ("2026-11-06T12:00:00.123456789Z WARN msg", warning, Some("2026-11-06T12:00:00.123456789Z")),
            ("2026-11-06T12:00:00+01:00 ERROR msg", error, Some("2026-11-06T12:00:00+01:00")),
            ("ERROR:root:msg", error, None),
            ("WARNING:app.db:slow", warning, None),
            ("2026-11-06 12:00:00,123 - name - ERROR - msg", error, Some("2026-11-06 12:00:00,123")),
            ("2026/11/06 12:00:00 [error] 29#29: msg", error, Some("2026/11/06 12:00:00")),
            ("2026/11/06 12:00:00 [warn] 29#29: msg", warning, Some("2026/11/06 12:00:00")),
            ("Nov  6 12:00:00 host proc[123]: connection failed", error, Some("Nov  6 12:00:00")),
            ("Nov  6 12:00:00 host proc[123]: warning: low disk", warning, Some("Nov  6 12:00:00")),
            ("Nov  6 12:00:00 host proc[123]: all fine", None, Some("Nov  6 12:00:00")),
        ];
        for (line, severity, time) in samples {
            assert_eq!(classify(line), (severity, time.map(str::to_owned)), "{line}");
        }
    }

    #[test]
    fn more_default_formats_colour_the_event() {
        assert!(kinds_on_line("2026-11-06T12:00:00Z ERROR msg\n", 0).contains(&("ERROR".to_owned(), Highlight::Keyword)));
        assert!(kinds_on_line("Nov  6 12:00:00 host proc[1]: boom failed\n", 0).contains(&("boom failed".to_owned(), Highlight::Keyword)));
    }

    const LARAVEL: &str = "\
[2026-11-06 10:00:00] local.ERROR: Division by zero {\"exception\":\"[object]\"}
[stacktrace]
#0 /var/www/app/Http/Foo.php(12): fn()
#1 {main}
[2026-11-06 10:00:01] local.INFO: fine
";

    #[test]
    fn laravel_lines_colour_as_error_with_their_stack_trace() {
        assert!(kinds_on_line(LARAVEL, 0).contains(&("ERROR".to_owned(), Highlight::Keyword)));
        assert_eq!(kinds_on_line(LARAVEL, 2).len(), 1);
        assert_eq!(kinds_on_line(LARAVEL, 2)[0].1, Highlight::Keyword);
        assert!(kinds_on_line(LARAVEL, 4).contains(&("INFO".to_owned(), Highlight::String)));
        assert_eq!(folding(&Config::default(), LARAVEL), [Fold { start_line: 0, end_line: 3, kind: FoldKind::Region }]);
    }

    #[test]
    fn monolog_and_serilog_lines_are_classified() {
        let error = Some(Severity::Error);
        let warning = Some(Severity::Warning);
        assert_eq!(
            classify("[2026-11-06T10:00:00.123456+00:00] app.ERROR: boom {\"ctx\":1} []"),
            (error, Some("2026-11-06T10:00:00.123456+00:00".to_owned()))
        );
        assert_eq!(
            classify("2026-11-06 10:00:00.123 +00:00 [ERR] boom"),
            (error, Some("2026-11-06 10:00:00.123 +00:00".to_owned()))
        );
        assert_eq!(classify("2026-11-06 10:00:00.123 +00:00 [WRN] hm").0, warning);
        assert_eq!(classify("2026-11-06 10:00:00.123 +00:00 [FTL] dead").0, error);
    }

    #[test]
    fn existing_formats_keep_their_detection() {
        let config = Config::default();
        let uuid = |text: &str| config.detect(text).map(|f| f.uuid.clone());
        assert_eq!(uuid("10:00:00.000|INFO|main|started\n").as_deref(), Some("b5772998-bf1e-4d9d-ab41-da0b86451163"));
        assert_eq!(uuid("05-01 10:00:00.000  1234  5678 E MyTag: failed\n").as_deref(), Some("b8fcb4d4-b1b8-4681-90f1-42f7c02aaf67"));
        assert_eq!(uuid("[10:00:00]W: [Step 1/2] odd\n").as_deref(), Some("e9fa2755-8390-42f5-a41e-a909c58c8cf9"));
        assert_eq!(uuid(IDEA).as_deref(), Some("8a0e8992-94cb-4f4c-8be2-42b03609626b"));
        assert!(uuid("Nov is a month\nplain text 12:00\n").is_none());
    }

    #[test]
    fn lines_are_coloured_by_level() {
        assert!(kinds_on_line(IDEA, 0).contains(&("INFO".to_owned(), Highlight::String)));
        assert!(kinds_on_line(IDEA, 1).contains(&("WARN".to_owned(), Highlight::Tag)));
        assert!(kinds_on_line(IDEA, 2).contains(&("ERROR".to_owned(), Highlight::Keyword)));
        assert!(kinds_on_line(IDEA, 5).is_empty(), "DEBUG has no default pattern");
    }

    #[test]
    fn time_column_of_a_highlighted_line_is_a_number() {
        assert_eq!(kinds_on_line(IDEA, 0)[0], ("2024-05-01 10:00:00,123 ".to_owned(), Highlight::Number));
    }

    #[test]
    fn stack_trace_lines_take_the_colour_of_their_event() {
        let trace = kinds_on_line(IDEA, 4);
        assert_eq!(trace, [("\tat com.example.Foo.bar(Foo.java:42)".to_owned(), Highlight::Keyword)]);
    }

    #[test]
    fn tokens_never_overlap_or_cross_lines() {
        let tokens = highlights(&Config::default(), IDEA);
        for pair in tokens.windows(2) {
            assert!(pair[0].range.end <= pair[1].range.start);
        }
        assert!(tokens.iter().all(|t| !IDEA[t.range.clone()].contains('\n')));
    }

    #[test]
    fn multi_line_events_fold() {
        assert_eq!(folding(&Config::default(), IDEA), [Fold { start_line: 2, end_line: 4, kind: FoldKind::Region }]);
    }

    #[test]
    fn outline_lists_errors_and_warnings() {
        let symbols = outline(&Config::default(), IDEA);
        let names: Vec<_> = symbols.iter().map(|s| (s.name.as_str(), s.detail.as_deref().unwrap())).collect();
        assert_eq!(
            names,
            [
                ("slow indexing", "WARN 2024-05-01 10:00:01,000"),
                ("card declined for user 42", "ERROR 2024-05-01 10:00:02,000")
            ]
        );
        for symbol in &symbols {
            assert_eq!(&IDEA[symbol.name_range.clone()], symbol.name);
        }
    }

    #[test]
    fn crlf_logs_are_parsed() {
        assert_eq!(outline(&Config::default(), &IDEA.replace('\n', "\r\n")).len(), 2);
    }

    #[test]
    fn big_logs_are_cut_at_a_line_break() {
        let text = IDEA.repeat(20_000);
        let cut = head(&text);
        assert!(cut.len() <= MAX_DOCUMENT_BYTES);
        assert!(cut.ends_with('\n'));
    }

    #[test]
    fn custom_patterns_come_from_the_setting() {
        let host = MockHost::install();
        let xml = r#"<LogHighlightingPattern pattern="^\s*debug\s*$" fg="-8355712" />"#;
        host.set_setting(CUSTOM_PATTERNS, &format!("{xml:?}"));
        let tokens = Ideolog::default().highlights("ideolog", IDEA);
        assert!(tokens.iter().any(|t| &IDEA[t.range.clone()] == "DEBUG" && t.kind == Highlight::Comment));
    }

    #[test]
    fn next_error_skips_to_the_next_error_event() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        open_log(&host, 0, 0);
        Ideolog::default().run_command("next-error", &host.invocation(), None).unwrap();
        assert_eq!(host.opened_locations(), [("/p/logs/idea.log".to_owned(), 2, 0)]);
    }

    #[test]
    fn entries_move_by_event_not_by_line() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        open_log(&host, 4, 3);
        let mut plugin = Ideolog::default();
        plugin.run_command("next-entry", &host.invocation(), None).unwrap();
        plugin.run_command("previous-entry", &host.invocation(), None).unwrap();
        assert_eq!(host.opened_locations(), [("/p/logs/idea.log".to_owned(), 5, 0), ("/p/logs/idea.log".to_owned(), 1, 0)]);
    }

    #[test]
    fn no_error_below_is_a_notification() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        open_log(&host, 5, 0);
        Ideolog::default().run_command("next-error", &host.invocation(), None).unwrap();
        assert!(host.opened_locations().is_empty());
        assert_eq!(host.notifications().len(), 1);
    }

    #[test]
    fn the_last_entry_says_so() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        open_log(&host, 5, 0);
        Ideolog::default().run_command("next-entry", &host.invocation(), None).unwrap();
        assert_eq!(host.notifications()[0].body, "Already at the last log entry.");
    }

    #[test]
    fn jump_to_source_follows_the_stack_frame_under_the_caret() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        host.add_file("src/main/java/com/example/Foo.java", "class Foo {}\n");
        host.add_file("src/test/java/other/Foo.java", "class Foo {}\n");
        open_log(&host, 4, 10);
        Ideolog::default().run_command("jump-to-source", &host.invocation(), None).unwrap();
        assert_eq!(host.opened_locations(), [("/p/src/main/java/com/example/Foo.java".to_owned(), 41, 0)]);
    }

    #[test]
    fn jump_to_source_finds_the_logging_line() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        host.add_file("src/Pay.kt", "fun pay() {\n    log.error(\"card declined for user $id\")\n}\n");
        host.add_file("src/Other.kt", "fun other() {\n    log.info(\"started\")\n}\n");
        open_log(&host, 2, 0);
        Ideolog::default().run_command("jump-to-source", &host.invocation(), None).unwrap();
        assert_eq!(host.opened_locations(), [("/p/src/Pay.kt".to_owned(), 1, 0)]);
    }

    #[test]
    fn clicking_a_stack_frame_goes_to_its_line() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        host.set_project_root(Some("/p"));
        host.add_file("src/main/java/com/example/Foo.java", "class Foo {}\n");
        let doc = Document { path: "/p/logs/idea.log".into(), language: "ideolog".into(), version: 1, text: IDEA.into() };
        let locations = Ideolog::default().definition(&doc, Position::new(4, 10));
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].path, "/p/src/main/java/com/example/Foo.java");
        assert_eq!(locations[0].range.start, Position::new(41, 0));
        assert!(Ideolog::default().definition(&doc, Position::new(0, 3)).is_empty());
    }

    #[test]
    fn commands_refuse_other_files() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        host.open_editor("/p/a.rs", "rust", "fn main() {}", &[(0, 0, 0, 0)]);
        let error = Ideolog::default().run_command("next-error", &host.invocation(), None).unwrap_err();
        assert_eq!(error.to_string(), "Not a log file.");
    }
}

