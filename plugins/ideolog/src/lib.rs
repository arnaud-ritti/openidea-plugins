//! Ideolog for OpenIDEA: log highlighting, folding of multi-line events and an
//! outline of errors and warnings (the `syntax-provider` of the `ideolog`
//! language), plus navigation commands.

mod log;
mod live_tail;
mod folding;
mod jvm;
mod pattern_editor;
mod patterns;
mod semantic;
mod source;
mod viewer;

use log::{Config, Event, Format, Parsed, Severity};
use openidea_plugin::providers::{Document, Providers};
use openidea_plugin::syntax::{Fold, FoldKind, Highlight, Symbol, SymbolKind, SyntaxProvider, Token};
use openidea_plugin::views::{View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    ConsoleRule, Error, Event as HostEvent, Invocation, Location, LogLevel, NotificationLevel, Plugin, Position, Project, StatusItem,
    StatusMenuEntry, TextEditor, host, text,
};
use std::ops::Range;
use std::rc::Rc;

const CUSTOM_PATTERNS: &str = "customPatterns";
const TIME_GAP: &str = "timeGapSeconds";
const CONSOLE_HIGHLIGHTING: &str = "consoleHighlighting";
const FORMAT_STATUS: &str = "format";
// ponytail: only the head of a big log is parsed, so the provider answers within
// the IDE's 50 ms edit budget; parse incrementally if big logs need colour past it.
const MAX_DOCUMENT_BYTES: usize = 512 << 10;
/// Highlighting stops this far into one event, so a log without recognised
/// headers (one giant event) cannot outrun the provider deadline.
const MAX_HIGHLIGHTED_EVENT_BYTES: usize = 32 << 10;

#[derive(Default)]
pub struct Ideolog {
    pattern_editor: pattern_editor::Editor,
    viewer: viewer::Viewer,
    jvm: jvm::Adapter,
    tail: live_tail::LiveTail,
    /// Path and project root of the log last marked with Mark Current Log Source.
    marked: Option<(String, String)>,
    /// The `customPatterns` setting it was built from, and the config.
    config: Option<(String, Rc<Config>)>,
    editor_denied: bool,
}

impl Ideolog {
    fn config(&mut self) -> Rc<Config> {
        let custom = patterns::combined(&host::setting_string(CUSTOM_PATTERNS).unwrap_or_default());
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

impl Ideolog {
    fn show_format(&mut self, ctx: &Invocation) {
        let Some(editor) = ctx.editor().filter(|e| matches!(e.language().as_str(), "ideolog" | "log")) else {
            host::remove_status_item(FORMAT_STATUS);
            return;
        };
        let Ok(document) = editor.document() else {
            return;
        };

        let config = self.config();
        let (mut text, mut tooltip, click) = match config.detect(head(&document.text)) {
            Some(format) => (format!("Log: {}", format.name), "The log format Ideolog detected.".to_owned(), "manage custom formats"),
            None => ("Log: plain text".to_owned(), "No known log format matches this file.".to_owned(), "add a custom format"),
        };
        text.push_str(&highlight_limit_note(&editor.language(), &document.text));
        if document.text.len() > MAX_DOCUMENT_BYTES {
            tooltip.push_str(" Folding, outline and stack-frame links are off for logs over 512 KiB.");
        }
        let item = |hint: &str| StatusItem { text: text.clone(), tooltip: Some(format!("{tooltip} {hint}")), command: Some("patterns".into()) };
        // Older IDEs refuse the menu: the item then opens Formats and Highlighting, as before.
        if host::set_status_item_menu(FORMAT_STATUS, &item("Click for log navigation and formats."), &status_menu()).is_err() {
            host::set_status_item(FORMAT_STATUS, &item(&format!("Click to {click}.")));
        }
    }
}

/// The format item's menu: the log commands that act on the focused log, then the tools.
fn status_menu() -> Vec<StatusMenuEntry> {
    vec![
        StatusMenuEntry::action("Go to Next Error", "next-error"),
        StatusMenuEntry::action("Go to Next Warning", "next-warning"),
        StatusMenuEntry::action("Go to Next Exception", "next-exception"),
        StatusMenuEntry::action("Jump to Source", "jump-to-source"),
        StatusMenuEntry::Separator,
        StatusMenuEntry::action("Capture Active Log Snapshot", "capture-log"),
        StatusMenuEntry::action("Open Live Log Tail", "live-tail"),
        StatusMenuEntry::Separator,
        StatusMenuEntry::action("Formats and Highlighting", "patterns"),
    ]
}

/// Run console rules: colour only the level words loggers print in capitals and the first line of
/// a Java or Python exception, never whole lines, so the IDE's own level colours stay. Each
/// pattern passes the host's backtracking check (one repetition, or anchored with `^`).
fn console_rules() -> Vec<ConsoleRule> {
    vec![
        ConsoleRule::new(r"\b(ERROR|FATAL|SEVERE|CRITICAL)\b").foreground("red").style("bold"),
        ConsoleRule::new(r"\b(WARN|WARNING)\b").foreground("yellow").style("bold"),
        ConsoleRule::new(r#"^(Caused by: |Exception in thread "[^"]*" )?[\w$.]+(Exception|Error)\b"#).foreground("red"),
        ConsoleRule::new(r"^Traceback \(most recent call last\):").foreground("red"),
    ]
}

/// Sets the console rules, or removes them when the setting is off.
fn apply_console_rules() {
    let rules = if host::setting_bool(CONSOLE_HIGHLIGHTING).unwrap_or(true) { console_rules() } else { Vec::new() };
    if let Err(error) = host::set_console_rules(&rules) {
        host::log(LogLevel::Warn, &format!("Ideolog: console highlighting unavailable: {error}"));
    }
}

/// Status-bar suffix saying how much of a big log gets colours, empty when all of it does.
fn highlight_limit_note(language: &str, text: &str) -> String {
    let highlighted = if language == "log" { semantic::bounded_head(text) } else { head(text) };
    if highlighted.len() == text.len() {
        return String::new();
    }
    match highlighted.lines().count() {
        0 => " (too large to highlight)".to_owned(),
        lines => format!(" (first {lines} lines highlighted)"),
    }
}

/// Persistent patterns are intentional global settings; no model/sample is sent.
fn refresh_changed_patterns(before: &str) {
    if patterns::stored() != before {
        ui_kit::web::refresh_view(patterns::VIEW, include_str!("ui_config.json"));
    }
}

impl Ideolog {
    /// Marks, the JVM snapshot and the live tail each belong to one log; events
    /// about other files or editors leave them alone.
    fn events_name_a_tracked_log(&self, events: &[HostEvent]) -> bool {
        let tracked: Vec<(String, String)> = [self.marked.clone(), self.jvm.source(), self.tail.source()]
            .into_iter()
            .flatten()
            .collect();
        let is_tracked = |path: &str| tracked.iter().any(|(log, _)| log == path);
        events.iter().any(|event| match event {
            HostEvent::FileOpened(path) | HostEvent::FileSaved(path) => is_tracked(path),
            HostEvent::EditorFocused(path) => path.as_deref().is_some_and(is_tracked),
            HostEvent::DocumentChanged(change) => is_tracked(&change.path),
            HostEvent::FilesChanged(changes) => changes
                .created
                .iter()
                .chain(&changes.modified)
                .chain(&changes.removed)
                .any(|path| is_tracked(path)),
            HostEvent::VcsChanged(root) => tracked.iter().any(|(_, tracked_root)| tracked_root == root),
            HostEvent::Other { kind, data } if kind == "file-closed" => {
                serde_json::from_str::<String>(data).is_ok_and(|path| is_tracked(&path))
            }
            _ => false,
        })
    }

    fn run_log_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        if id == "extend-log-selection" { self.extend_log_selection(ctx)?; return Ok(None); }
        if id == "live-tail" { ui_kit::web::reveal_view(live_tail::VIEW, include_str!("ui_config.json"))?; return Ok(None); }
        if id == "stop-tail" { self.stop_tail(); return Ok(None); }
        if id == "capture-jvm" { self.capture_jvm(ctx)?; return Ok(None); }
        if id == "mark-log-source" { semantic::mark(self, ctx)?; return Ok(None); }
        if id == "clear-log-source" { host::clear_decorations(semantic::DECORATION_SET)?; return Ok(None); }
        if id == "capture-log" {self.capture_log(ctx)?;return Ok(None);}
        if id == "log-snapshot" {ui_kit::web::reveal_view(viewer::VIEW, include_str!("ui_config.json"))?;return Ok(None);}
        if id == "patterns" {
            ui_kit::web::reveal_view(patterns::VIEW, include_str!("ui_config.json"))?;
            return Ok(None);
        }

        if id == "remove-highlightings" {
            let count = patterns::highlighting_count();
            if count == 0 {
                host::notify(NotificationLevel::Info, "Ideolog", "There are no custom highlighting patterns to remove.");
                return Ok(None);
            }
            let body = format!("Delete all {count} custom highlighting pattern(s)? Custom log formats are kept.");
            if !host::confirm("Remove Custom Highlightings", &body)? {
                return Ok(None);
            }
            let before = patterns::stored();
            let removed = patterns::remove_highlightings()?;
            self.config();
            refresh_changed_patterns(&before);
            host::notify(NotificationLevel::Info, "Ideolog", &format!("Removed {removed} highlighting pattern(s)."));
            return Ok(None);
        }

        let Some(editor) = ctx.editor() else {
            return Err(if self.editor_denied { Error::Denied("editor.read".into()) } else { Error::Invalid("No file is open.".into()) });
        };
        if !matches!(editor.language().as_str(), "ideolog" | "log") {
            return Err("Not a log file.".into());
        }

        if id == "highlight-selection" {
            let before = patterns::stored();
            let text = selected_text(&editor)?;
            patterns::highlight_text(&text)?;
            self.config();
            refresh_changed_patterns(&before);
            return Ok(None);
        }

        let config = self.config();
        match id {
            "jump-to-source" => jump_to_source(&config, ctx, &editor),
            "next-error" => step(&config, &editor, "error", true),
            "next-warning" => step(&config, &editor, "warning", true),
            "next-exception" => step(&config, &editor, "exception", true),
            "next-entry" => go_to(&config, &editor, "Already at the last log entry.", |events, at| events.get(at + 1)),
            "previous-entry" => {
                go_to(&config, &editor, "Already at the first log entry.", |events, at| at.checked_sub(1).map(|i| &events[i]))
            }
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }
}

impl Plugin for Ideolog {
    fn activate(&mut self, ctx: &openidea_plugin::Context) -> Result<(), Error> {
        self.editor_denied = !ctx.has("editor.read");
        // Compile bounded stored regex configuration outside editor requests.
        self.config();
        apply_console_rules();
        Ok(())
    }
    fn deactivate(&mut self) {
        self.stop_tail();
        let _ = host::clear_decorations(semantic::DECORATION_SET);
    }
    fn handle_events(&mut self, events: &[HostEvent], ctx: &Invocation) {
        if events.iter().any(|e| matches!(e, HostEvent::Other { kind, .. } if kind == "settings-changed")) {
            self.config();
            apply_console_rules();
        }
        if self.events_name_a_tracked_log(events) {
            self.tail_lifecycle(ctx);
            self.invalidate_jvm_tokens();
            let _ = host::clear_decorations(semantic::DECORATION_SET);
        } else if events.iter().any(|e| matches!(e, HostEvent::Scheduled)) {
            self.tail_lifecycle(ctx);
        }
        if events.iter().any(|e| matches!(e, HostEvent::EditorFocused(_) | HostEvent::FileOpened(_))) {
            self.show_format(ctx);
        }
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        let result = self.run_log_command(id, ctx, arg);
        if !self.editor_denied {
            return result;
        }

        result.map_err(|e| ui_kit::denied::map_capability(e, "Log file commands", "editor.read", "Ideolog"))
    }

}

impl SyntaxProvider for Ideolog {
    fn highlights(&mut self, language: &str, text: &str) -> Vec<Token> {
        if language!="ideolog" {return Vec::new();}
        let gap = host::setting_number(TIME_GAP).unwrap_or(0.0);
        highlights_with_gaps(&self.config(), head(text), gap)
    }

    fn outline(&mut self, language: &str, text: &str) -> Vec<Symbol> {
        if language!="ideolog" {return Vec::new();}
        outline(&self.config(), head(text))
    }

    fn folding(&mut self, language: &str, text: &str) -> Vec<Fold> {
        if language!="ideolog" {return Vec::new();}
        folding(&self.config(), head(text))
    }
}

impl Providers for Ideolog {
    /// Cmd+click on a stack frame opens its file and line.
    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        if !matches!(doc.language.as_str(), "log" | "ideolog") || doc.text.len()>MAX_DOCUMENT_BYTES {return Vec::new();}
        let Some(offset) = text::offset(&doc.text, at) else {
            return Vec::new();
        };
        if text::position(&doc.text,offset)!=at {return Vec::new();}
        let line_start = doc.text[..offset].rfind('\n').map_or(0, |i| i + 1);
        let line = log::lines(&doc.text[line_start..]).next().map_or("", |(_, l)| l);
        let column = offset - line_start;
        let Some(frame) = source::frames(line).into_iter().find(|f| f.span.contains(&column)) else {
            return Vec::new();
        };
        let Some(project) = project_of(&doc.path) else {
            return Vec::new();
        };
        let Ok(Some(path)) = source::resolve(&project, &frame) else {
            return Vec::new();
        };

        let target = Position::new(frame.line.saturating_sub(1), 0);
        vec![Location { path, range: openidea_plugin::Range { start: target, end: target } }]
    }
}

impl Views for Ideolog {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, _ctx: &Invocation) -> Result<View, Error> {
        (|| -> Result<View, Error> {
            match view {
                live_tail::VIEW => Ok(self.render_tail(_ctx)),
                "jvm-observations" => self.render_jvm(_ctx),
                viewer::VIEW => self.render_log(_viewport,_ctx),
                patterns::VIEW => Ok(patterns::render(&self.pattern_editor)),
                _ => Err(Error::NotFound(view.into())),
            }
        })().map(ui_kit::actions::available)
    }

    fn handle(&mut self, view: &str, event: ViewEvent, _ctx: &Invocation) -> Result<(), Error> {
        if ui_kit::web::handle(self, view, &event, _ctx, include_str!("ui_config.json"))? { return Ok(()); }
        if view==live_tail::VIEW { return self.handle_tail(event,_ctx); }
        if view=="jvm-observations" { return self.handle_jvm(event,_ctx); }
        if view==viewer::VIEW {return self.handle_log(event,_ctx);}
        if view == LEVELS_PANEL {
            return match event {
                ViewEvent::Message(request) => self.levels_panel(&request, _ctx),
                _ => Ok(()),
            };
        }
        let result = match view {
            patterns::VIEW => patterns::handle(&mut self.pattern_editor, event),
            _ => Err(Error::NotFound(view.into())),
        };
        if result.is_ok() {
            self.config();
        }
        result
    }
}

pub const LEVELS_PANEL: &str = "levels";

impl Ideolog {
    /// The editor panel beside a log. `{op:"summary",id,path}` answers the detected format and
    /// event counts (broadcast to every panel frame, so `id` names the asking one);
    /// `{op:"go",path,level,forward}` moves the caret like the navigation commands;
    /// `{op:"snapshot",path,level}` captures the log into the Log Snapshot table filtered to `level`
    /// (the editor cannot fold or hide lines for a plugin); `{op:"patterns"}` opens the Formats &
    /// Highlighting view.
    fn levels_panel(&mut self, request: &str, ctx: &Invocation) -> Result<(), Error> {
        let request: serde_json::Value = serde_json::from_str(request).map_err(|_| Error::Invalid("Malformed panel request.".into()))?;
        if request["op"] == "patterns" {
            return ui_kit::web::reveal_view(patterns::VIEW, include_str!("ui_config.json"));
        }
        let path = request["path"].as_str().ok_or_else(|| Error::Invalid("The panel sent no file.".into()))?;
        let editor = ctx.editor().filter(|editor| editor.path() == path && matches!(editor.language().as_str(), "ideolog" | "log"));
        let config = self.config();
        match request["op"].as_str() {
            Some("go") => {
                let Some(editor) = editor else {
                    host::notify(NotificationLevel::Info, "Ideolog", "Click in this log first: the panel moves the caret of the focused editor.");
                    return Ok(());
                };
                let level = request["level"].as_str().unwrap_or_default();
                step(&config, &editor, level, request["forward"].as_bool().unwrap_or(true)).map(|_| ())
            }
            Some("snapshot") => {
                if editor.is_none() {
                    host::notify(NotificationLevel::Info, "Ideolog", "Click in this log first: the snapshot captures the focused editor.");
                    return Ok(());
                }
                match self.capture_log(ctx) {
                    Ok(()) => {
                        self.viewer.level = viewer::level_filter(request["level"].as_str().unwrap_or_default());
                        ui_kit::web::refresh_view(viewer::VIEW, include_str!("ui_config.json"));
                    }
                    Err(Error::Cancelled) => {}
                    Err(error) => host::notify(NotificationLevel::Info, "Ideolog", &error.to_string()),
                }
                Ok(())
            }
            Some("summary") => {
                let text = match editor {
                    Some(editor) => editor.document().map(|document| document.text),
                    None => project_of(path).ok_or(Error::Stale).and_then(|project| project.read_file(path)),
                };
                // Every request is answered, so the page never waits in vain.
                let mut reply = match text {
                    Ok(text) => summary(&config, &text),
                    Err(error) => serde_json::json!({ "message": format!("Cannot read this log ({error}). Click in it to count its events.") }),
                };
                reply["id"] = request["id"].as_str().filter(|id| id.len() <= 64).into();
                reply["path"] = path.into();
                host::post_view_message(LEVELS_PANEL, &reply.to_string())
            }
            _ => Err(Error::Invalid("Unknown panel request.".into())),
        }
    }
}

/// The format and event counts of the part of `text` that gets colours ([`head`]).
fn summary(config: &Config, text: &str) -> serde_json::Value {
    let counted = head(text);
    let (format, events) = parsed_events(config, counted);
    let mut levels = [0; 5];
    let mut exceptions = 0;
    for (event, parsed) in &events {
        let level = parsed.level(format);
        if let Some(level) = level {
            levels[level as usize] += 1;
        }
        if log::has_exception(event.text, level == Some(Severity::Error)) {
            exceptions += 1;
        }
    }
    // A level the format cannot express is null, and the panel hides its row.
    let count = |level: Severity| format.is_none_or(|f| f.expresses(level)).then_some(levels[level as usize]);
    serde_json::json!({
        "format": format.map(|format| format.name.as_str()),
        "error": count(Severity::Error),
        "warning": count(Severity::Warning),
        "exception": exceptions,
        "info": count(Severity::Info),
        "debug": count(Severity::Debug),
        "trace": count(Severity::Trace),
        "entry": events.len(),
        "firstLines": (counted.len() < text.len()).then(|| counted.lines().count()),
    })
}

/// Moves the caret to the previous or next event of `level` (`error`, `warning`, `info`, `debug`,
/// `trace`, `exception` or `entry`).
fn step(config: &Config, editor: &TextEditor, level: &str, forward: bool) -> Result<Option<String>, Error> {
    type Wanted = fn(&(Range<u32>, Option<Severity>, bool)) -> bool;
    let (noun, wanted): (&str, Wanted) = match level {
        "error" => ("errors", |(_, severity, _)| *severity == Some(Severity::Error)),
        "warning" => ("warnings", |(_, severity, _)| *severity == Some(Severity::Warning)),
        "info" => ("info entries", |(_, severity, _)| *severity == Some(Severity::Info)),
        "debug" => ("debug entries", |(_, severity, _)| *severity == Some(Severity::Debug)),
        "trace" => ("trace entries", |(_, severity, _)| *severity == Some(Severity::Trace)),
        "exception" => ("exceptions", |(_, _, exception)| *exception),
        "entry" => ("entries", |_| true),
        _ => return Err(Error::Invalid(format!("Unknown log level {level}."))),
    };
    let none = format!("No more {noun} {} the caret.", if forward { "below" } else { "above" });
    go_to(config, editor, &none, |events, at| match forward {
        true => events.iter().skip(at + 1).find(|entry| wanted(entry)),
        false => events[..at].iter().rev().find(|entry| wanted(entry)),
    })
}

/// The open project holding `path` (providers get no window context).
fn project_of(path: &str) -> Option<Project> {
    if !path.starts_with('/') || path.contains('\\') || path.chars().any(char::is_control) || path.split('/').skip(1).any(|p|p.is_empty() || matches!(p,"."|"..")) {return None;}
    let project=host::project_of(path)?;
    let root=project.root().ok()?;
    path.strip_prefix(&format!("{}/",root.trim_end_matches('/')))?;
    Some(project)
}

openidea_plugin::register_plugin!(Ideolog, providers, views, syntax, semantic_tokens, language_features);

/// The document up to the last line break within [`MAX_DOCUMENT_BYTES`].
fn head(text: &str) -> &str {
    if text.len() <= MAX_DOCUMENT_BYTES {
        return text;
    }

    let cut = log::floor_char_boundary(text, MAX_DOCUMENT_BYTES);
    text[..cut].rfind('\n').map_or("", |end| &text[..=end])
}

fn parsed_events<'a>(config: &'a Config, text: &'a str) -> (Option<&'a Format>, Vec<(Event<'a>, Parsed<'a>)>) {
    let format = config.detect(text);
    let events = log::events(format, text).into_iter().map(|e| {
        let parsed = Parsed::new(format, e.text);
        (e, parsed)
    });
    (format, events.collect())
}

#[cfg(test)]
fn highlights(config: &Config, text: &str) -> Vec<Token> {
    highlights_with_gaps(config, text, 0.0)
}

/// With `gap_seconds` above zero, the time of an event that comes at least
/// that long after the previous timed one gets the `Constant` colour.
fn highlights_with_gaps(config: &Config, text: &str, gap_seconds: f64) -> Vec<Token> {
    highlights_limited(config, text, gap_seconds, usize::MAX, None)
}

/// Semantic tokens share the original highlighting rules, with tighter live
/// request limits. The custom language keeps its existing lexical path.
fn highlights_limited(config: &Config, text: &str, gap_seconds: f64, cap: usize, deadline: Option<std::time::Instant>) -> Vec<Token> {
    highlights_checked(config,text,gap_seconds,cap,||deadline.is_some_and(|end|std::time::Instant::now()>=end))
}
fn highlights_checked(config: &Config, text: &str, gap_seconds: f64, cap: usize, mut expired: impl FnMut() -> bool) -> Vec<Token> {
    if cap==0 { return Vec::new(); }
    let mut checkpoint=||if expired() {Err(())} else {Ok(())};
    if checkpoint().is_err() { return Vec::new(); }
    let Ok(format)=config.detect_checked(text,&mut checkpoint) else { return Vec::new(); };
    let Ok(events)=log::events_checked(format,text,&mut checkpoint) else { return Vec::new(); };
    let mut out = Vec::new();
    let mut previous: Option<f64> = None;
    for event in &events {
        if out.len() >= cap || checkpoint().is_err() { break; }
        let event_text=capped_event(event.text);
        let parsed=Parsed::new(format,event_text);
        if checkpoint().is_err() { break; }
        let now = parsed.time(format).and_then(|time| log::time_seconds(time, format.and_then(|f| f.time_pattern.as_deref())));
        let gap_range = match (gap_seconds > 0.0, previous, now) {
            (true, Some(before), Some(now)) if now - before >= gap_seconds => parsed.time_range(format),
            _ => None,
        };
        previous = now.or(previous);

        let highlighted=config.highlight(format, &parsed);
        if checkpoint().is_err() { break; }
        for (range, kind) in highlighted {
            match &gap_range {
                Some(gap) if range.start < gap.end && gap.start < range.end => {}
                _ => push_by_line(&mut out, event_text, event.offset, range, kind),
            }
        }
        if let Some(gap) = gap_range {
            push_by_line(&mut out, event_text, event.offset, gap, Highlight::Constant);
        }
    }
    out.truncate(cap);
    out
}

fn capped_event(event: &str) -> &str {
    if event.len() <= MAX_HIGHLIGHTED_EVENT_BYTES {
        return event;
    }

    let head = &event[..log::floor_char_boundary(event, MAX_HIGHLIGHTED_EVENT_BYTES)];
    head.rfind('\n').map_or(head, |end| &head[..=end])
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
        let Some(severity) = parsed.severity(format) else {
            continue;
        };

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
    let at = text::offset(text, selection.start).ok_or(Error::Stale)?;
    if text::position(text,at)!=selection.start {return Err(Error::Stale);}
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    Ok((selection.start.line, at - line_start))
}

type Entries = [(Range<u32>, Option<Severity>, bool)];

/// Moves the caret to the start of the event `pick` chooses, given the
/// events and the index of the one at the caret.
fn go_to(
    config: &Config,
    editor: &TextEditor,
    none: &str,
    pick: impl FnOnce(&Entries, usize) -> Option<&(Range<u32>, Option<Severity>, bool)>,
) -> Result<Option<String>, Error> {
    let text = editor.document()?.text;
    let (line, _) = caret(editor, &text)?;
    let (format, events) = parsed_events(config, &text);
    let entries: Vec<_> = events
        .iter()
        .map(|(e, p)| {
            let level = p.level(format);
            (e.lines.clone(), level, log::has_exception(e.text, level == Some(Severity::Error)))
        })
        .collect();
    let at = entries.iter().position(|(lines, ..)| lines.contains(&line)).unwrap_or(entries.len().saturating_sub(1));

    let Some((lines, ..)) = pick(&entries, at) else {
        host::notify(NotificationLevel::Info, "Ideolog", none);
        return Ok(None);
    };
    host::open_location(&editor.path(), lines.start, 0)?;
    Ok(None)
}

/// A stack frame on the caret line (the one under the caret first), else the
/// source line that logged the event.
fn selected_text(editor: &TextEditor) -> Result<String, Error> {
    let document = editor.document()?;
    let selection = editor.selections()?.into_iter().next().ok_or("Select the text to highlight.")?;
    let start = text::offset(&document.text, selection.start);
    let end = text::offset(&document.text, selection.end);
    let selected = start.zip(end).and_then(|(a, b)| document.text.get(a..b)).unwrap_or_default();
    if selected.trim().is_empty() || selected.contains('\n') {
        return Err("Select some text on one line to highlight.".into());
    }
    Ok(selected.to_owned())
}

fn jump_to_source(config: &Config, ctx: &Invocation, editor: &TextEditor) -> Result<Option<String>, Error> {
    let invoking = ctx.project().ok_or("No project is open.")?;
    let project=project_of(&editor.path()).ok_or(Error::Stale)?;
    if invoking.root()?!=project.root()? {return Err(Error::Stale);}
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
    let Some((event, parsed)) = events.iter().find(|(e, _)| e.lines.contains(&line)) else {
        return Ok(None);
    };
    let message = parsed.message();
    if message.is_empty() {
        return Err("This event has no message to search for.".into());
    }

    match source::find_logging_line(&project, message, parsed.category(format), event.text)? {
        Some((path, line)) => host::open_location(&path, line, 0)?,
        None => {
            host::notify(NotificationLevel::Info, "Ideolog", "No source line found for this event. Open the project that produced the log.")
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::views::Body;

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
    fn long_pauses_colour_the_time_of_the_next_event() {
        let text = "2024-05-01 10:00:00,000 [   1] INFO - #a - one\n2024-05-01 10:00:01,000 [   1] INFO - #a - two\n2024-05-01 10:05:00,000 [   1] INFO - #a - three\n";
        let marked = |gap| -> Vec<String> {
            highlights_with_gaps(&Config::default(), text, gap)
                .into_iter()
                .filter(|t| t.kind == Highlight::Constant)
                .map(|t| text[t.range].trim().to_owned())
                .collect()
        };
        assert_eq!(marked(60.0), ["2024-05-01 10:05:00,000"]);
        assert!(marked(0.0).is_empty());
        assert_eq!(marked(1.0).len(), 2);
    }

    #[test]
    fn status_item_names_the_detected_format() {
        let host = MockHost::install();
        open_log(&host, 0, 0);
        let mut plugin = Ideolog::default();

        plugin.handle_events(&[HostEvent::EditorFocused(Some("/p/logs/idea.log".into()))], &host.invocation());
        assert_eq!(host.status_items()["format"].text, "Log: IntelliJ IDEA");

        host.open_editor("/p/logs/other.log", "ideolog", "just some text\n", &[]);
        plugin.handle_events(&[HostEvent::EditorFocused(None)], &host.invocation());
        assert_eq!(host.status_items()["format"].text, "Log: plain text");

        host.open_editor("/p/a.rs", "rust", "fn main() {}", &[]);
        plugin.handle_events(&[HostEvent::EditorFocused(None)], &host.invocation());
        assert!(host.status_items().is_empty());
    }

    #[test]
    fn status_item_menu_offers_declared_log_commands() {
        let host = MockHost::install();
        open_log(&host, 0, 0);
        Ideolog::default().handle_events(&[HostEvent::EditorFocused(None)], &host.invocation());
        let menu = host.status_menu("format");
        assert_eq!(menu, status_menu());
        let manifest = include_str!("../plugin.toml");
        for entry in menu {
            if let StatusMenuEntry::Action { command, .. } = entry {
                assert!(manifest.contains(&format!("id = \"{command}\"")), "{command} is not declared");
            }
        }
        assert!(host.status_items()["format"].tooltip.as_deref().unwrap().ends_with("Click for log navigation and formats."));
    }

    #[test]
    fn console_rules_follow_the_setting() {
        let host = MockHost::install();
        let mut plugin = Ideolog::default();
        plugin.activate(&openidea_plugin::Context::default()).unwrap();
        assert_eq!(host.console_rules(), console_rules());
        assert!(host.console_rules().iter().all(|rule| !rule.whole_line && !rule.ignore_case));

        host.set_setting(CONSOLE_HIGHLIGHTING, "false");
        plugin.handle_events(&[HostEvent::Other { kind: "settings-changed".into(), data: String::new() }], &host.invocation());
        assert!(host.console_rules().is_empty());

        host.set_setting(CONSOLE_HIGHLIGHTING, "true");
        plugin.handle_events(&[HostEvent::Other { kind: "settings-changed".into(), data: String::new() }], &host.invocation());
        assert_eq!(host.console_rules().len(), 4);
    }

    #[test]
    fn console_rules_colour_levels_and_exceptions_only() {
        let rules: Vec<_> = console_rules().into_iter().map(|rule| fancy_regex::Regex::new(&rule.pattern).unwrap()).collect();
        let hits = |line: &str| rules.iter().filter(|rule| rule.is_match(line).unwrap()).count();
        for line in [
            "2024-05-01 10:00:02,000 ERROR c.e.Boom - card declined",
            "[WARN] slow indexing",
            "java.lang.IllegalStateException: boom",
            "Caused by: java.io.IOException: closed",
            "Exception in thread \"main\" java.lang.OutOfMemoryError: heap",
            "Traceback (most recent call last):",
            "ValueError: invalid literal",
        ] {
            assert_eq!(hits(line), 1, "{line}");
        }
        for line in ["INFO started in 2 s", "no errors found", "NO_ERROR code", "Error: plain message", "\tat com.example.Foo.bar(Foo.java:42)"] {
            assert_eq!(hits(line), 0, "{line}");
        }
    }

    #[test]
    fn status_item_says_when_a_big_log_is_only_partly_highlighted() {
        let host = MockHost::install();
        let mut plugin = Ideolog::default();
        host.open_editor("/p/big.log", "log", &"INFO line\n".repeat(60_000), &[]);
        plugin.handle_events(&[HostEvent::EditorFocused(None)], &host.invocation());
        let text = &host.status_items()["format"].text;
        assert!(text.starts_with("Log: plain text (first ") && text.ends_with(" lines highlighted)"), "{text}");

        host.open_editor("/p/small.log", "log", "INFO line\n", &[]);
        plugin.handle_events(&[HostEvent::EditorFocused(None)], &host.invocation());
        assert_eq!(host.status_items()["format"].text, "Log: plain text");
    }

    #[test]
    fn patterns_view_adds_a_format_and_removes_it() {
        let host = MockHost::install();
        let mut plugin = Ideolog::default();
        let ctx = host.invocation();
        let click = |plugin: &mut Ideolog, id: &str| { let id = if id.starts_with("pattern-") { plugin.pattern_editor.control_key(id) } else { id.into() }; plugin.handle("patterns", ViewEvent::Button(id), &ctx).unwrap(); };

        click(&mut plugin, "add-format");
        for (key, value) in [("name", "Mine"), ("pattern", r"^(\d+) \[(\w+)\] (.*)$"), ("linePattern", r"^\d")] {
            plugin.handle("patterns", ViewEvent::Field(plugin.pattern_editor.control_key(&format!("pattern-{key}")), value.into()), &ctx).unwrap();
        }
        click(&mut plugin, "pattern-save");

        let text = "12 [WARN] careful\n";
        let format_name = |plugin: &mut Ideolog| plugin.config().detect(text).map(|f| f.name.clone());
        assert_eq!(format_name(&mut plugin).as_deref(), Some("Mine"));

        click(&mut plugin, "add-highlight");
        plugin.handle("patterns", ViewEvent::Field(plugin.pattern_editor.control_key("pattern-pattern"), "^x".into()), &ctx).unwrap();
        click(&mut plugin, "pattern-save");
        let Body::Table(table) = plugin.render("patterns", None, &ctx).unwrap().body else { panic!("a table") };
        assert_eq!(table.rows.len(), 2);

        host.answer_next_prompt(Some(&["0"]));
        click(&mut plugin, "remove");
        assert_eq!(format_name(&mut plugin), None);
    }

    #[test]
    fn a_broken_pattern_is_not_stored() {
        let host = MockHost::install();
        let ctx = host.invocation();
        let mut plugin = Ideolog::default();
        plugin.handle("patterns", ViewEvent::Button("add-format".into()), &ctx).unwrap();
        plugin.handle("patterns", ViewEvent::Field(plugin.pattern_editor.control_key("pattern-pattern"), "^(unclosed".into()), &ctx).unwrap();
        plugin.handle("patterns", ViewEvent::Button(plugin.pattern_editor.control_key("pattern-save")), &ctx).unwrap();
        assert!(patterns::stored().is_empty());
        let Body::Form(fields) = plugin.render("patterns", None, &ctx).unwrap().body else { panic!("keep invalid draft open") };
        assert!(fields.iter().any(|field| field.key.starts_with("pattern-preview-output@") && format!("{:?}", field.value).contains("Invalid regex")));
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
        assert_eq!(classify("2026-11-06 10:00:00.123 +00:00 [ERR] boom"), (error, Some("2026-11-06 10:00:00.123 +00:00".to_owned())));
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
            [("slow indexing", "WARN 2024-05-01 10:00:01,000"), ("card declined for user 42", "ERROR 2024-05-01 10:00:02,000")]
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
    fn registered_custom_syntax_refuses_wrong_language_and_truncated_first_line() {
        let _h=MockHost::install().with_capabilities(&[]);let mut plugin=Ideolog::default();
        assert!(plugin.highlights("rust",IDEA).is_empty());assert!(plugin.outline("log",IDEA).is_empty());assert!(plugin.folding("rust",IDEA).is_empty());assert!(plugin.config.is_none(),"wrong language performs no configuration preparation");
        let source=format!("ERROR {}","😀".repeat(MAX_DOCUMENT_BYTES));
        assert!(plugin.highlights("ideolog",&source).is_empty(),"partial source line must not become event");assert!(plugin.outline("ideolog",&source).is_empty());assert!(plugin.folding("ideolog",&source).is_empty());
        assert_eq!(head("ERROR complete final line"),"ERROR complete final line");
        let source=format!("ERROR complete\r\n{}","😀".repeat(MAX_DOCUMENT_BYTES));assert_eq!(head(&source),"ERROR complete\r\n");
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
    fn log_commands_denied_names_permission() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        open_log(&host, 0, 0);
        let mut plugin = Ideolog::default();
        plugin.activate(&host.context("aritti.ideolog")).unwrap();
        let error = plugin.run_command("next-error", &host.invocation(), None).unwrap_err().to_string();
        assert!(error.contains("needs the editor.read permission"), "{error}");
        assert!(error.contains("Settings › Plugins › Ideolog"), "{error}");
    }

    #[test]
    fn log_commands_without_an_editor_still_say_no_file_is_open() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        let mut plugin = Ideolog::default();
        plugin.activate(&host.context("aritti.ideolog")).unwrap();
        let error = plugin.run_command("next-error", &host.invocation(), None).unwrap_err().to_string();
        assert!(error.contains("No file is open."), "{error}");
    }

    #[test]
    fn next_error_skips_to_the_next_error_event() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        open_log(&host, 0, 0);
        Ideolog::default().run_command("next-error", &host.invocation(), None).unwrap();
        assert_eq!(host.opened_locations(), [("/p/logs/idea.log".to_owned(), 2, 0)]);
    }

    #[test]
    fn next_warning_targets_the_next_warn_event() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        open_log(&host, 0, 0);
        Ideolog::default().run_command("next-warning", &host.invocation(), None).unwrap();
        assert_eq!(host.opened_locations(), [("/p/logs/idea.log".to_owned(), 1, 0)]);
    }

    #[test]
    fn next_exception_skips_errors_without_one() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        let log = "2024-05-01 10:00:00,000 [   1]  INFO - #a.B - fine\n2024-05-01 10:00:00,123 [   1] ERROR - #a.B - plain failure\n2024-05-01 10:00:01,000 [   1] ERROR - #a.B - java.io.IOException: x\n";
        host.set_project_root(Some("/p"));
        host.open_editor("/p/a.log", "ideolog", log, &[(0, 0, 0, 0)]);
        let mut plugin = Ideolog::default();
        plugin.run_command("next-error", &host.invocation(), None).unwrap();
        plugin.run_command("next-exception", &host.invocation(), None).unwrap();
        assert_eq!(host.opened_locations(), [("/p/a.log".to_owned(), 1, 0), ("/p/a.log".to_owned(), 2, 0)]);
    }

    #[test]
    fn next_exception_targets_the_next_event_with_an_exception() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        open_log(&host, 0, 0);
        Ideolog::default().run_command("next-exception", &host.invocation(), None).unwrap();
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

    fn panel(plugin: &mut Ideolog, host: &MockHost, request: serde_json::Value) {
        plugin.handle(LEVELS_PANEL, ViewEvent::Message(request.to_string()), &host.invocation()).unwrap();
    }

    #[test]
    fn levels_panel_counts_events_and_names_the_format() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        open_log(&host, 0, 0);
        let mut plugin = Ideolog::default();
        panel(&mut plugin, &host, serde_json::json!({"op":"summary","id":"f1","path":"/p/logs/idea.log"}));
        let (view, reply) = host.view_messages().pop().unwrap();
        let reply: serde_json::Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(view, LEVELS_PANEL);
        assert_eq!(
            reply,
            serde_json::json!({"id":"f1","path":"/p/logs/idea.log","format":"IntelliJ IDEA","error":1,"warning":1,"exception":1,"info":1,"debug":1,"trace":0,"entry":4,"firstLines":null})
        );
    }

    #[test]
    fn levels_panel_says_how_much_of_a_big_log_it_counted() {
        let big = IDEA.repeat(MAX_DOCUMENT_BYTES / IDEA.len() + 2);
        let reply = summary(&Config::default(), &big);
        assert_eq!(reply["firstLines"].as_u64(), Some(head(&big).lines().count() as u64));
        assert!(reply["firstLines"].as_u64().unwrap() < big.lines().count() as u64);
    }

    #[test]
    fn levels_panel_moves_to_the_previous_and_next_event_of_a_level() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        open_log(&host, 5, 0);
        let mut plugin = Ideolog::default();
        panel(&mut plugin, &host, serde_json::json!({"op":"go","path":"/p/logs/idea.log","level":"warning","forward":false}));
        panel(&mut plugin, &host, serde_json::json!({"op":"go","path":"/p/logs/idea.log","level":"error","forward":true}));
        assert_eq!(host.opened_locations(), [("/p/logs/idea.log".to_owned(), 1, 0)]);
        assert_eq!(host.notifications()[0].body, "No more errors below the caret.");
        assert!(plugin.handle(LEVELS_PANEL, ViewEvent::Message(r#"{"op":"go","path":"/p/logs/idea.log","level":"x"}"#.into()), &host.invocation()).is_err());
    }

    #[test]
    fn levels_panel_steps_through_info_debug_and_trace() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        open_log(&host, 5, 0);
        let mut plugin = Ideolog::default();
        let go = |plugin: &mut Ideolog, level: &str, forward: bool| {
            panel(plugin, &host, serde_json::json!({"op":"go","path":"/p/logs/idea.log","level":level,"forward":forward}));
        };
        go(&mut plugin, "info", false);
        go(&mut plugin, "debug", false);
        go(&mut plugin, "trace", true);
        assert_eq!(host.opened_locations(), [("/p/logs/idea.log".to_owned(), 0, 0)]);
        assert_eq!(host.notifications().iter().map(|n| n.body.as_str()).collect::<Vec<_>>(), ["No more debug entries above the caret.", "No more trace entries below the caret."]);
    }

    #[test]
    fn levels_panel_hides_levels_the_format_cannot_express() {
        let python = summary(&Config::default(), "DEBUG:app:one\nINFO:app:two\nWARNING:app:three\n");
        assert_eq!((python["info"].as_u64(), python["debug"].as_u64(), python["trace"].as_u64()), (Some(1), Some(1), None));
        let syslog = summary(&Config::default(), "May  1 10:00:00 host app[1]: started\nMay  1 10:00:01 host app[1]: failed\n");
        assert_eq!(syslog["format"], "Syslog");
        assert_eq!((syslog["error"].as_u64(), syslog["info"].as_u64(), syslog["trace"].as_u64()), (Some(1), None, None));
        let serilog = summary(&Config::default(), "2024-05-01 10:00:00.000 +02:00 [VRB] a\n2024-05-01 10:00:00.000 +02:00 [INF] b\n");
        assert_eq!((serilog["info"].as_u64(), serilog["debug"].as_u64(), serilog["trace"].as_u64()), (Some(1), Some(0), Some(1)));
    }

    #[test]
    fn levels_panel_opens_the_snapshot_filtered_to_a_level() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        host.set_project_root(Some("/p"));
        host.add_file("logs/idea.log", IDEA);
        open_log(&host, 0, 0);
        let mut plugin = Ideolog::default();
        panel(&mut plugin, &host, serde_json::json!({"op":"snapshot","path":"/p/logs/idea.log","level":"debug"}));
        let view = plugin.render_log(None, &host.invocation()).unwrap();
        let openidea_plugin::views::Body::Table(table) = &view.body else { panic!("table") };
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0][1], "DEBUG");
        let sections: Vec<_> = view.toolbar.iter().filter(|b| b.id.starts_with("section:")).map(|b| b.label.as_str()).collect();
        assert_eq!(sections, ["All (4)", "Errors (1)", "Warnings (1)", "Info (1)", "\u{25cf} Debug (1)"]);
        plugin.handle(viewer::VIEW, ViewEvent::Button("section:all".into()), &host.invocation()).unwrap();
        let openidea_plugin::views::Body::Table(table) = plugin.render_log(None, &host.invocation()).unwrap().body else { panic!("table") };
        assert_eq!(table.total_rows, 4);
    }

    #[test]
    fn levels_panel_moves_only_the_focused_log_and_opens_the_patterns_view() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        open_log(&host, 0, 0);
        let mut plugin = Ideolog::default();
        panel(&mut plugin, &host, serde_json::json!({"op":"go","path":"/p/logs/other.log","level":"error","forward":true}));
        assert!(host.opened_locations().is_empty());
        assert_eq!(host.notifications().len(), 1);
        panel(&mut plugin, &host, serde_json::json!({"op":"patterns"}));
        assert_eq!(host.revealed_views().len(), 1);
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
    fn activation_warms_config_without_editor_or_project_grants() {
        let host = MockHost::install().with_capabilities(&["storage"]);
        let mut plugin = Ideolog::default();
        plugin.activate(&host.context("aritti.ideolog")).unwrap();
        assert!(plugin.config.is_some());
        assert!(host.process_runs().is_empty());
        assert!(host.requests().is_empty());
    }
    #[test]
    fn highlight_selection_adds_the_escaped_text_as_a_pattern() {
        let host = MockHost::install().with_capabilities(&["editor.read", "storage"]);
        host.open_editor("/p/a.log", "ideolog", "x a.b(1) y\n", &[(0, 2, 0, 8)]);
        host.answer_next_prompt(Some(&["HIGHLIGHT_MATCH"]));
        host.answer_next_prompt(Some(&["green"]));
        let mut plugin = Ideolog::default();
        plugin.activate(&host.context("aritti.ideolog")).unwrap();
        plugin.run_command("highlight-selection", &host.invocation(), None).unwrap();
        assert_eq!(plugin.config.as_ref().unwrap().0, patterns::combined(""));
        let stored = patterns::stored();
        assert!(stored.contains("pattern=\"a\\.b\\(1\\)\""), "{stored}");
        assert!(stored.contains("HIGHLIGHT_MATCH"));
        host.answer_next_prompt(None);
        Ideolog::default().run_command("remove-highlightings", &host.invocation(), None).unwrap();
        assert_eq!(patterns::stored(), stored, "dismissing the confirmation keeps the patterns");
        host.answer_next_prompt(Some(&[]));
        Ideolog::default().run_command("remove-highlightings", &host.invocation(), None).unwrap();
        assert!(patterns::stored().trim().is_empty());
    }

    #[test]
    fn remove_highlightings_asks_first() {
        let host = MockHost::install();
        patterns::save(&[patterns::highlight_element("seed", "HIGHLIGHT_LINE", -39836)]).unwrap();
        host.answer_next_prompt(Some(&[]));
        Ideolog::default().run_command("remove-highlightings", &host.invocation(), None).unwrap();
        assert_eq!(patterns::highlighting_count(), 0);
    }

    #[test]
    fn cancel_keeps_rules() {
        let host = MockHost::install();
        patterns::save(&[patterns::highlight_element("seed", "HIGHLIGHT_LINE", -39836)]).unwrap();
        host.answer_next_prompt(None);
        Ideolog::default().run_command("remove-highlightings", &host.invocation(), None).unwrap();
        assert_eq!(patterns::highlighting_count(), 1);
    }

    #[test]
    fn highlight_selection_needs_a_selection() {
        let host = MockHost::install().with_capabilities(&["editor.read", "storage"]);
        host.open_editor("/p/a.log", "ideolog", "abc\n", &[(0, 1, 0, 1)]);
        assert!(Ideolog::default().run_command("highlight-selection", &host.invocation(), None).is_err());
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
    fn registered_frame_navigation_refuses_unowned_prefix_collision_and_ambiguous_sources() {
        let h=MockHost::install().with_capabilities(&["project.read"]);h.set_project_root(Some("/p"));
        h.add_file("src/Foo.java","class Foo {}\n");h.add_file("longer/other/Foo.java","class Foo {}\n");
        let source="😀 at demo.Foo.run(Foo.java:7)\r\n";
        let mut d=Document {path:"/outside/log.txt".into(),language:"log".into(),version:1,text:source.into()};
        let at=text::position(source,source.find("Foo.java").unwrap());
        assert!(Ideolog::default().definition(&d,at).is_empty(),"unowned log must not borrow first project");
        d.path="/prefix/log.txt".into();assert!(Ideolog::default().definition(&d,at).is_empty(),"root prefix is not owner");
        d.path="/p/log.txt".into();assert!(Ideolog::default().definition(&d,at).is_empty(),"ambiguous suffix score must not pick shorter source");
    }
    #[test]
    fn registered_definition_keeps_utf16_nested_owner_and_request_bounds() {
        let h=MockHost::install().with_capabilities(&["project.read"]);h.set_project_root(Some("/p"));h.add_project("/p/nested");
        h.add_file("/p/src/demo/Foo.java","outer");h.add_file("/p/nested/src/demo/Foo.java","inner");
        let source="😀 preface\r\n😀 at demo.Foo.run(Foo.java:7)\r\n";
        let mut d=Document {path:"/p/nested/log.txt".into(),language:"log".into(),version:1,text:source.into()};
        let at=text::position(source,source.find("Foo.java").unwrap());
        let found=Ideolog::default().definition(&d,at);assert_eq!(found.len(),1);assert_eq!(found[0].path,"/p/nested/src/demo/Foo.java");assert_eq!(found[0].range.start,Position::new(6,0));
        assert!(Ideolog::default().definition(&d,Position::new(1,1)).is_empty(),"surrogate midpoint is invalid");
        d.language="rust".into();assert!(Ideolog::default().definition(&d,at).is_empty());d.language="log".into();
        d.text=format!("{}at demo.Foo.run(Foo.java:7)"," ".repeat(8193));assert!(Ideolog::default().definition(&d,Position::new(0,8200)).is_empty());
        d.text=format!("{source}{}","x".repeat(MAX_DOCUMENT_BYTES));assert!(Ideolog::default().definition(&d,at).is_empty());
    }
    #[test]
    fn registered_frame_navigation_refuses_capped_source_inventory_and_invalid_lines() {
        let h=MockHost::install().with_capabilities(&["project.read"]);h.set_project_root(Some("/p"));
        for n in 0..200 {h.add_file(&format!("/p/f{n}/Foo.java"),"class Foo {}\n");}
        let source="at f0.Foo.run(Foo.java:3)";
        let mut d=Document {path:"/p/log.txt".into(),language:"log".into(),version:1,text:source.into()};
        assert!(Ideolog::default().definition(&d,Position::new(0,15)).is_empty(),"capped list cannot establish unique best");
        for number in ["0","99999999999999999999999"] {d.text=format!("at f0.Foo.run(Foo.java:{number})");assert!(Ideolog::default().definition(&d,Position::new(0,15)).is_empty());}
    }
    #[test]
    fn source_command_refuses_foreign_editor_owner_and_invalid_utf16_caret() {
        let h=MockHost::install().with_capabilities(&["project.read","editor.read"]);h.set_project_root(Some("/p"));h.add_project("/other");
        h.add_file("/p/demo/Foo.java","class Foo {}\n");
        h.open_editor("/other/log.txt","log","at demo.Foo.run(Foo.java:3)",&[(0,16,0,16)]);
        assert!(Ideolog::default().run_command("jump-to-source",&h.invocation(),None).is_err(),"foreign editor must not search invoking project");assert!(h.opened_locations().is_empty());
        h.open_editor("/p/log.txt","log","😀 at demo.Foo.run(Foo.java:3)",&[(0,1,0,1)]);
        assert!(Ideolog::default().run_command("jump-to-source",&h.invocation(),None).is_err(),"invalid UTF16 caret must not become byte zero");assert!(h.opened_locations().is_empty());
    }
    #[test]
    fn fallback_source_command_excludes_foreign_owners_and_refuses_equal_message_rank() {
        let h=MockHost::install().with_capabilities(&["project.read","editor.read"]);h.set_project_root(Some("/p"));h.add_project("/p/aaa");
        h.add_file("/p/aaa/Pay.kt","log.error(\"application startup failed\")");h.add_file("/p/src/Pay.kt","log.error(\"application startup failed\")");
        h.open_editor("/p/log.txt","log","ERROR application startup failed\n",&[(0,0,0,0)]);
        Ideolog::default().run_command("jump-to-source",&h.invocation(),None).unwrap();assert_eq!(h.opened_locations(),[("/p/src/Pay.kt".into(),0,0)]);
        h.add_file("/p/second/Pay.kt","log.error(\"application startup failed\")");
        Ideolog::default().run_command("jump-to-source",&h.invocation(),None).unwrap();assert_eq!(h.opened_locations().len(),1,"equal rank must not create another attributed source jump");
    }
    #[test]
    fn registered_outline_and_next_error_keep_numeric_level_suffixes_unclassified() {
        let h=MockHost::install().with_capabilities(&["project.read","editor.read"]);h.set_project_root(Some("/p"));
        let source="2024/05/01 10:00:00 [info] 1#2: start\n2024/05/01 10:00:01 [error2] 1#2: ordinary custom level\n2024/05/01 10:00:02 [error] 1#2: actual error\n";
        let symbols=Ideolog::default().outline("ideolog",source);assert_eq!(symbols.len(),1,"error2 is not ERROR");assert!(symbols[0].name.contains("actual error"));
        h.open_editor("/p/log.txt","log",source,&[(0,0,0,0)]);Ideolog::default().run_command("next-error",&h.invocation(),None).unwrap();assert_eq!(h.opened_locations(),[("/p/log.txt".into(),2,0)]);
    }
    #[test]
    fn commands_refuse_other_files() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        host.open_editor("/p/a.rs", "rust", "fn main() {}", &[(0, 0, 0, 0)]);
        let error = Ideolog::default().run_command("next-error", &host.invocation(), None).unwrap_err();
        assert_eq!(error.to_string(), "Not a log file.");
    }
}

#[cfg(test)]
mod external_patterns_refresh_tests {
    use super::*;
    use openidea_plugin::{mock::MockHost, views::{Body,FieldValue}};
    fn select(host:&MockHost){
        host.open_editor("/project/private.log","ideolog","selected_literal",&[(0,0,0,16)]);
        host.answer_next_prompt(Some(&["HIGHLIGHT_MATCH"]));host.answer_next_prompt(Some(&["green"]));
    }
    #[test]
    fn selected_editor_text_becomes_global_definition_not_a_preview_sample(){
        let host=MockHost::install();let mut plugin=Ideolog::default();select(&host);
        plugin.run_command("highlight-selection",&host.invocation(),None).unwrap();
        assert!(patterns::stored().contains("selected_literal"));
        assert_eq!(host.refreshed_views(),[patterns::VIEW.to_owned()]);assert!(host.view_messages().iter().all(|(_,text)| serde_json::from_str::<serde_json::Value>(text).ok().is_some_and(|value| value["channel"] == "ui-kit-refresh" && value.as_object().unwrap().len() <= 2)));
        assert!(host.view_messages().iter().all(|(_,text)|!text.contains("selected_literal")));
        host.set_project_root(None);
        let global=plugin.render(patterns::VIEW,None,&host.invocation()).unwrap();assert!(format!("{:?}",global.body).contains("selected_literal"));
        plugin.handle(patterns::VIEW,ViewEvent::Button("add-highlight".into()),&host.invocation()).unwrap();
        let Body::Form(fields)=plugin.render(patterns::VIEW,None,&host.invocation()).unwrap().body else {panic!("draft expected")};
        let sample=fields.iter().find(|f|f.key.starts_with("pattern-sample@")).unwrap();
        assert!(matches!(&sample.value,FieldValue::Multiline(text) if text.contains("Example failure") && !text.contains("selected_literal")));
    }
    #[test]
    fn changed_remove_notifies_patterns_but_empty_cancel_and_error_do_not(){
        let host=MockHost::install();let mut plugin=Ideolog::default();
        plugin.run_command("remove-highlightings",&host.invocation(),None).unwrap();assert!(host.view_messages().is_empty());
        host.open_editor("/project/a.log","ideolog","selected_literal",&[(0,0,0,16)]);
        host.answer_next_prompt(None);plugin.run_command("highlight-selection",&host.invocation(),None).unwrap();assert!(host.view_messages().is_empty());
        host.open_editor("/project/a.log","ideolog","selected_literal",&[(0,0,0,0)]);
        assert!(plugin.run_command("highlight-selection",&host.invocation(),None).is_err());assert!(host.view_messages().is_empty());
        patterns::save(&[patterns::highlight_element("seed","HIGHLIGHT_LINE",-39836)]).unwrap();
        host.answer_next_prompt(Some(&[]));
        plugin.run_command("remove-highlightings",&host.invocation(),None).unwrap();
        assert_eq!(host.refreshed_views(),[patterns::VIEW.to_owned()]);assert!(host.view_messages().iter().all(|(_,text)| serde_json::from_str::<serde_json::Value>(text).ok().is_some_and(|value| value["channel"] == "ui-kit-refresh" && value.as_object().unwrap().len() <= 2)));assert!(host.refreshed_views().iter().all(|view|view==patterns::VIEW));
    }
    #[test]
    fn global_manual_draft_survives_external_change_and_save_conflict(){
        let host=MockHost::install();let mut plugin=Ideolog::default();
        patterns::save(&[patterns::highlight_element("seed","HIGHLIGHT_LINE",-39836)]).unwrap();
        plugin.handle(patterns::VIEW,ViewEvent::Button("add-highlight".into()),&host.invocation()).unwrap();
        let sample=plugin.pattern_editor.control_key("pattern-sample");
        plugin.handle(patterns::VIEW,ViewEvent::Field(sample,"manual-global-settings-sample".into()),&host.invocation()).unwrap();
        host.answer_next_prompt(Some(&[]));
        plugin.run_command("remove-highlightings",&host.invocation(),None).unwrap();
        host.set_project_root(Some("/other"));
        assert!(format!("{:?}",plugin.render(patterns::VIEW,None,&host.invocation()).unwrap().body).contains("manual-global-settings-sample"));
        let save=plugin.pattern_editor.control_key("pattern-save");plugin.handle(patterns::VIEW,ViewEvent::Button(save),&host.invocation()).unwrap();
        let model=format!("{:?}",plugin.render(patterns::VIEW,None,&host.invocation()).unwrap().body);
        assert!(model.contains("changed elsewhere"));assert!(model.contains("manual-global-settings-sample"));assert!(patterns::stored().is_empty());
    }
    #[test]
    fn custom_global_render_uses_correlated_bridge_without_process_or_network(){
        let host=MockHost::install();host.set_project_root(None);let mut plugin=Ideolog::default();
        for sequence in [1,2]{
            let before=host.view_messages().len();
            let request=format!(r#"{{"channel":"ui-kit","frameId":"0123456789abcdef0123456789abcdef","sequence":{sequence},"project":null,"op":"render"}}"#);
            plugin.handle(patterns::VIEW,ViewEvent::Message(request.to_string()),&host.invocation()).unwrap();
            assert_eq!(host.view_messages().len(),before + 1);
            let answer: serde_json::Value = serde_json::from_str(&host.view_messages().last().unwrap().1).unwrap();
            assert_eq!(answer["model"]["activeView"], patterns::VIEW);
            assert_eq!(answer["model"]["viewTabs"].as_array().unwrap().len(), 4);
            assert!(matches!(plugin.render(patterns::VIEW,None,&host.invocation()).unwrap().body,Body::Message(_)));
        }
        assert!(host.requests().is_empty());assert!(host.process_runs().is_empty());
    }
}
