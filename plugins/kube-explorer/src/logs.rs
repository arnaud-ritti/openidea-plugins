//! A log snapshot in the panel: one `kubectl logs --tail=N --timestamps` run,
//! filtered in the plugin by text or regular expression, case and start time,
//! with soft wrap, hideable time and source fields, a copy of the shown lines and a download
//! of them through `tee`. Following a log stays a terminal command: a view cannot stream.

use openidea_plugin::views::{Body, Button, View};
use openidea_plugin::{Error, NotificationLevel, host};
use regex_lite::{Regex, RegexBuilder, escape};

use crate::{KubeExplorer, REQUEST_TIMEOUT, TEE, WORKLOADS, escape_html, failure, file_safe};

const DEFAULT_TAIL: f64 = 1000.0;
const MAX_LOG_BYTES: usize = 2 << 20;
const MAX_SHOWN_LINES: usize = 5000;

pub struct LogView {
    title: String,
    kind: String,
    row: String,
    lines: Vec<String>,
    truncated: bool,
    filter: String,
    regex: bool,
    match_case: bool,
    since: String,
    until: String,
    wrap: bool,
    show_time: bool,
    show_source: bool,
}

struct Shown<'a> {
    lines: Vec<&'a str>,
    matcher: Option<Regex>,
    problem: Option<String>,
}

fn split_source(line: &str) -> (&str, &str) {
    match line.strip_prefix('[').and_then(|rest| rest.find("] ")) {
        Some(close) => (&line[..close + 3], &line[close + 3..]),
        None => ("", line),
    }
}

fn timestamp(line: &str) -> Option<&str> {
    let token = split_source(line).1.split(' ').next()?;
    let digits = token.get(..4)?;
    (digits.bytes().all(|b| b.is_ascii_digit()) && token.as_bytes().get(4) == Some(&b'-') && token.contains('T')).then_some(token)
}

fn cap(text: String) -> (String, bool) {
    if text.len() <= MAX_LOG_BYTES {
        return (text, false);
    }
    let mut from = text.len() - MAX_LOG_BYTES;
    while !text.is_char_boundary(from) {
        from += 1;
    }
    let from = text[from..].find('\n').map_or(from, |newline| from + newline + 1);
    (text[from..].to_owned(), true)
}

impl LogView {
    fn shown(&self) -> Shown<'_> {
        let mut problem = None;
        let matcher = match self.filter.is_empty() {
            true => None,
            false => {
                let pattern = if self.regex { self.filter.clone() } else { escape(&self.filter) };
                match RegexBuilder::new(&pattern).case_insensitive(!self.match_case).size_limit(1 << 20).build() {
                    Ok(matcher) => Some(matcher),
                    Err(error) => {
                        problem = Some(format!("invalid regular expression: {error}"));
                        None
                    }
                }
            }
        };
        let since = self.since.trim().replace(' ', "T");
        let until = self.until.trim().replace(' ', "T");
        let mut in_range = since.is_empty();
        let mut lines = Vec::new();
        for line in &self.lines {
            if let Some(stamp) = timestamp(line) {
                let after = since.is_empty() || stamp[..since.len().min(stamp.len())] >= since[..since.len().min(stamp.len())];
                let before = until.is_empty() || stamp[..until.len().min(stamp.len())] <= until[..until.len().min(stamp.len())];
                in_range = after && before;
            }
            if in_range && matcher.as_ref().is_none_or(|m| m.is_match(line)) {
                lines.push(line.as_str());
            }
        }
        if lines.len() > MAX_SHOWN_LINES {
            lines.drain(..lines.len() - MAX_SHOWN_LINES);
        }
        Shown { lines, matcher, problem }
    }

    fn display(&self, line: &str) -> String {
        let (source, rest) = split_source(line);
        let rest = match (self.show_time, timestamp(line)) {
            (false, Some(stamp)) => rest[stamp.len()..].trim_start(),
            _ => rest,
        };
        match self.show_source {
            true => format!("{source}{rest}"),
            false => rest.to_owned(),
        }
    }

    fn displayed(&self, shown: &Shown) -> String {
        shown.lines.iter().map(|line| self.display(line)).collect::<Vec<_>>().join("\n")
    }

    fn page(&self, shown: &Shown) -> String {
        let body: Vec<String> = shown
            .lines
            .iter()
            .map(|raw| {
                let line = self.display(raw);
                let line = line.as_str();
                match &shown.matcher {
                None => escape_html(line),
                Some(matcher) => {
                    let mut out = String::new();
                    let mut at = 0;
                    for found in matcher.find_iter(line).filter(|m| !m.is_empty()) {
                        out.push_str(&escape_html(&line[at..found.start()]));
                        out.push_str(&format!("<mark>{}</mark>", escape_html(found.as_str())));
                        at = found.end();
                    }
                    out.push_str(&escape_html(&line[at..]));
                    out
                }
                }
            })
            .collect();
        let white_space = if self.wrap { "pre-wrap" } else { "pre" };
        format!(
            "<style>:root{{--bg:Canvas;--fg:CanvasText}}body{{margin:0;background:var(--openidea-bg,var(--bg));color:var(--openidea-fg,var(--fg))}}\
             pre{{margin:0;padding:8px 10px;font:12px/1.5 var(--openidea-font-mono,ui-monospace,Menlo,monospace);white-space:{white_space};word-break:break-all}}\
             mark{{background:color-mix(in srgb,var(--openidea-warning,#d6ae58) 55%,transparent);color:inherit}}</style><pre>{}</pre>\
             <script>window.openidea&&openidea.onMessage(function(m){{if(m==='end')window.scrollTo(0,document.documentElement.scrollHeight)}})</script>",
            body.join("\n")
        )
    }

    pub fn render(&self) -> View {
        let shown = self.shown();
        let mut status = format!("{}: {} of {} lines", self.title, shown.lines.len(), self.lines.len());
        if self.truncated {
            status.push_str(" (older output cut)");
        }
        if let Some(problem) = &shown.problem {
            status = format!("{status}; {problem}");
        }
        let toggle = |label: &str, on: bool| format!("{label}: {}", if on { "on" } else { "off" });
        View::new(Body::Html(self.page(&shown)))
            .status(&status)
            .button(Button::new("back", "Back").icon("arrow-left"))
            .button(Button::new("log-filter", "Filter…").icon("search"))
            .button(Button::new("log-since", "Since…").icon("history"))
            .button(Button::new("log-until", "Until…"))
            .button(Button::new("log-regex", &toggle("Regex", self.regex)))
            .button(Button::new("log-case", &toggle("Match case", self.match_case)))
            .button(Button::new("log-wrap", &toggle("Wrap", self.wrap)))
            .button(Button::new("log-time", &toggle("Time", self.show_time)))
            .button(Button::new("log-source", &toggle("Source", self.show_source)))
            .button(Button::new("log-end", "Scroll to end").icon("arrow-down"))
            .button(Button::new("log-clear", "Clear").icon("trash"))
            .button(Button::new("log-refresh", "Refresh").icon("refresh"))
            .button(Button::new("log-copy", "Copy").icon("copy"))
            .button(Button::new("log-download", "Download…").icon("download"))
    }
}

/// Inside the project only: a shared directory such as /tmp would make the file readable by every local user.
fn suggested_path(root: Option<&str>, file: &str) -> Option<String> {
    root.map(|root| format!("{}/{file}", root.trim_end_matches('/')))
}

impl KubeExplorer {
    fn download_log(&self) -> Result<(), Error> {
        let Some(view) = &self.log_view else { return Ok(()) };
        let shown = view.shown();
        let root = host::projects().ok().and_then(|projects| projects.first().and_then(|project| project.root().ok()));
        let file = format!("{}.log", file_safe(&format!("{}-{}", view.kind, view.row)));
        let suggestion = suggested_path(root.as_deref(), &file);
        let prompt = "Write the shown lines to (absolute path, overwritten; logs can hold sensitive data, so choose a folder only you can read)";
        let Some(path) = host::input(prompt, suggestion.as_deref())? else { return Ok(()) };
        let path = path.trim();
        if !path.starts_with('/') {
            return Err("Enter an absolute path.".into());
        }
        let mut text = view.displayed(&shown);
        text.push('\n');
        let out = match host::run_process(TEE, &[path.to_owned()], Some(&text)) {
            Ok(out) => out,
            Err(Error::Denied(_)) => {
                host::notify(NotificationLevel::Info, "Download logs", "Allow the optional tee capability for Kube Explorer to write the shown lines to a file.");
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        if out.status != Some(0) {
            return Err(failure(TEE, &out.stderr, out.status));
        }
        let _ = host::open_location(path, 0, 0);
        crate::config::notify_success("Logs saved", &format!("{} lines in {path}", shown.lines.len()));
        Ok(())
    }

    fn fetch_log_lines(&self, kind: &str, row: &str) -> Result<(Vec<String>, bool), Error> {
        let (namespace, name) = self.split_scope(kind, row)?;
        let tail = host::setting_number("logViewerLines").unwrap_or(DEFAULT_TAIL).clamp(10.0, 20000.0) as u32;
        let tail = format!("--tail={tail}");
        let target = format!("{kind}/{name}");
        let args: Vec<&str> = match kind {
            "pods" => vec!["logs", &name, "--timestamps", "--prefix", "--all-containers", &tail, REQUEST_TIMEOUT],
            _ => vec!["logs", &target, "--all-pods=true", "--all-containers", "--max-log-requests=20", "--timestamps", "--prefix", &tail, REQUEST_TIMEOUT],
        };
        let out = self.kubectl_in(namespace.as_deref(), &args, None)?;
        let (text, truncated) = cap(out);
        Ok((text.lines().map(str::to_owned).collect(), truncated))
    }

    pub fn open_log_view(&mut self, kind: &str, row: &str) -> Result<(), Error> {
        if kind != "pods" && !WORKLOADS.contains(&kind) {
            return Err(format!("Logs do not apply to {kind}.").into());
        }
        let _task = host::begin_task(&format!("Reading logs of {kind}/{row}"), false);
        let (lines, truncated) = self.fetch_log_lines(kind, row)?;
        let previous = self.log_view.take();
        self.log_view = Some(LogView {
            title: format!("Logs {kind}/{row}"),
            kind: kind.to_owned(),
            row: row.to_owned(),
            lines,
            truncated,
            filter: previous.as_ref().map(|v| v.filter.clone()).unwrap_or_default(),
            regex: previous.as_ref().is_some_and(|v| v.regex),
            match_case: previous.as_ref().is_some_and(|v| v.match_case),
            since: String::new(),
            until: String::new(),
            wrap: previous.as_ref().is_none_or(|v| v.wrap),
            show_time: previous.as_ref().is_none_or(|v| v.show_time),
            show_source: previous.as_ref().is_none_or(|v| v.show_source),
        });
        self.details = None;
        host::refresh_view(crate::TREE);
        Ok(())
    }

    pub fn log_button(&mut self, button: &str) -> Result<(), Error> {
        let Some(view) = &mut self.log_view else { return Ok(()) };
        match button {
            "log-filter" => {
                let Some(text) = host::input("Show lines matching (empty: all)", Some(&view.filter))? else { return Ok(()) };
                view.filter = text.trim().to_owned();
            }
            "log-since" => {
                let Some(text) = host::input("Show lines from this time on, e.g. 2025-11-06 or 2025-11-06T10:30 (UTC, empty: all)", Some(&view.since))?
                else {
                    return Ok(());
                };
                view.since = text.trim().to_owned();
            }
            "log-until" => {
                let Some(text) = host::input("Show lines up to this time, e.g. 2025-11-06T12:00 (UTC, empty: all)", Some(&view.until))? else {
                    return Ok(());
                };
                view.until = text.trim().to_owned();
            }
            "log-time" => view.show_time = !view.show_time,
            "log-source" => view.show_source = !view.show_source,
            "log-end" => return host::post_view_message(crate::TREE, "end"),
            "log-clear" => view.lines.clear(),
            "log-download" => return self.download_log(),
            "log-regex" => view.regex = !view.regex,
            "log-case" => view.match_case = !view.match_case,
            "log-wrap" => view.wrap = !view.wrap,
            "log-copy" => {
                let shown = view.shown();
                let text = view.displayed(&shown);
                match host::copy_to_clipboard(&text) {
                    Ok(()) => crate::config::notify_success("Logs copied", &format!("{} lines", shown.lines.len())),
                    Err(Error::Denied(_)) => host::notify(
                        NotificationLevel::Info,
                        "Copy logs",
                        "Allow the optional clipboard capability to copy the shown lines.",
                    ),
                    Err(error) => return Err(error),
                }
                return Ok(());
            }
            "log-refresh" => {
                let (kind, row) = (view.kind.clone(), view.row.clone());
                let (lines, truncated) = self.fetch_log_lines(&kind, &row)?;
                if let Some(view) = &mut self.log_view {
                    view.lines = lines;
                    view.truncated = truncated;
                }
            }
            _ => return Ok(()),
        }
        host::refresh_view(crate::TREE);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::ProcessOutput;

    use crate::KUBECTL;

    use super::*;

    const LOG: &str = "2025-11-06T09:00:00.1Z start <ok>\n2025-11-06T10:00:00.2Z ERROR boom 42\ncontinued without stamp\n2025-11-06T11:00:00.3Z info Done\n2025-11-07T00:00:01Z error again\n";

    fn view(filter: &str) -> LogView {
        LogView {
            title: "Logs pods/web".into(),
            kind: "pods".into(),
            row: "web".into(),
            lines: LOG.lines().map(str::to_owned).collect(),
            truncated: false,
            filter: filter.into(),
            regex: false,
            match_case: false,
            since: String::new(),
            until: String::new(),
            wrap: true,
            show_time: true,
            show_source: true,
        }
    }

    fn texts(view: &LogView) -> Vec<String> {
        view.shown().lines.iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn text_filter_ignores_case_until_match_case_is_on() {
        let mut v = view("error");
        assert_eq!(texts(&v).len(), 2);
        v.match_case = true;
        assert_eq!(texts(&v), ["2025-11-07T00:00:01Z error again"]);
    }

    #[test]
    fn plain_text_is_not_a_pattern_but_regex_mode_is() {
        let mut v = view("boom 4.");
        assert!(texts(&v).is_empty());
        v.regex = true;
        assert_eq!(texts(&v).len(), 1);
        v.filter = "(".into();
        assert_eq!(texts(&v).len(), 5);
        assert!(v.shown().problem.unwrap().contains("invalid regular expression"));
    }

    #[test]
    fn since_keeps_lines_from_that_time_and_stampless_lines_follow_their_predecessor() {
        let mut v = view("");
        v.since = "2025-11-06 10:00".into();
        assert_eq!(texts(&v).len(), 4);
        assert!(texts(&v)[1].starts_with("continued"));
        v.since = "2025-11-07".into();
        assert_eq!(texts(&v), ["2025-11-07T00:00:01Z error again"]);
    }

    #[test]
    fn the_page_escapes_html_marks_matches_and_wraps_on_request() {
        let mut v = view("ok");
        let page = v.page(&v.shown());
        assert!(page.contains("start &lt;<mark>ok</mark>&gt;") && page.contains("pre-wrap"));
        v.wrap = false;
        assert!(v.page(&v.shown()).contains("white-space:pre;"));
    }

    #[test]
    fn output_beyond_the_cap_keeps_the_newest_whole_lines() {
        let line = "x".repeat(99) + "\n";
        let (kept, truncated) = cap(line.repeat(MAX_LOG_BYTES / 100 + 50));
        assert!(truncated && kept.len() <= MAX_LOG_BYTES && kept.ends_with('\n') && kept.lines().all(|l| l.len() == 99));
        assert!(!cap("short\n".into()).1);
    }

    fn host(output: &str) -> (MockHost, std::rc::Rc<std::cell::RefCell<Vec<Vec<String>>>>) {
        let host = MockHost::install().with_capabilities(&["process:kubectl", "storage", "clipboard"]);
        let runs: std::rc::Rc<std::cell::RefCell<Vec<Vec<String>>>> = Default::default();
        let seen = runs.clone();
        let output = output.to_owned();
        host.on_process(KUBECTL, move |args, _| {
            seen.borrow_mut().push(args.to_vec());
            Ok(ProcessOutput { status: Some(0), stdout: output.clone(), stderr: String::new() })
        });
        (host, runs)
    }

    #[test]
    fn opening_reads_one_snapshot_and_the_buttons_filter_and_copy_it() {
        let (host, runs) = host(LOG);
        host.set_setting("logViewerLines", "50");
        let mut kube = KubeExplorer { namespace: Some("shop".into()), ..Default::default() };
        kube.open_log_view("pods", "web").unwrap();
        assert_eq!(runs.borrow()[0], ["--namespace=shop", "logs", "web", "--timestamps", "--prefix", "--all-containers", "--tail=50", REQUEST_TIMEOUT]);

        host.answer_next_prompt(Some(&["error"]));
        kube.log_button("log-filter").unwrap();
        kube.log_button("log-copy").unwrap();
        assert_eq!(host.clipboard().unwrap().lines().count(), 2);

        host.answer_next_prompt(None);
        kube.log_button("log-filter").unwrap();
        assert_eq!(kube.log_view.as_ref().unwrap().filter, "error");

        kube.log_button("log-refresh").unwrap();
        assert_eq!(runs.borrow().len(), 2);
        kube.button("back").unwrap();
        assert!(kube.log_view.is_none());
    }

    #[test]
    fn a_workload_reads_all_its_pods_and_copy_without_the_capability_only_notifies() {
        let (_host, runs) = host(LOG);
        let mut kube = KubeExplorer { namespace: Some("shop".into()), ..Default::default() };
        kube.open_log_view("deployments", "web").unwrap();
        assert!(runs.borrow()[0].contains(&"deployments/web".to_owned()) && runs.borrow()[0].contains(&"--all-pods=true".to_owned()));

        let denied = MockHost::install().with_capabilities(&["process:kubectl", "storage"]);
        kube.log_button("log-copy").unwrap();
        assert_eq!(denied.notifications().len(), 1);
    }

    #[test]
    fn other_kinds_and_bad_rows_run_nothing() {
        let (_host, runs) = host(LOG);
        let mut kube = KubeExplorer::default();
        assert!(kube.open_log_view("services", "web").is_err());
        kube.namespace = Some("*".into());
        assert!(kube.open_log_view("pods", "--bad").is_err());
        assert!(runs.borrow().is_empty());
    }

    const PREFIXED: &str = "[pod/web/app] 2025-11-06T09:00:00Z one\n[pod/web/side] 2025-11-06T10:00:00Z two\n[pod/web/app] 2025-11-06T11:00:00Z three\n";

    fn prefixed() -> LogView {
        LogView { lines: PREFIXED.lines().map(str::to_owned).collect(), ..view("") }
    }

    #[test]
    fn until_bounds_the_range_and_the_fields_can_be_hidden() {
        let mut v = prefixed();
        v.since = "2025-11-06T09:30".into();
        v.until = "2025-11-06T10:30".into();
        assert_eq!(texts(&v), ["[pod/web/side] 2025-11-06T10:00:00Z two"]);
        v.show_time = false;
        assert_eq!(v.display(texts(&v)[0].as_str()), "[pod/web/side] two");
        v.show_source = false;
        assert_eq!(v.display(texts(&v)[0].as_str()), "two");
    }

    #[test]
    fn clear_scroll_and_field_buttons_change_the_view() {
        let (host, _) = host(PREFIXED);
        let mut kube = KubeExplorer::default();
        kube.open_log_view("pods", "web").unwrap();
        kube.log_button("log-source").unwrap();
        assert!(!kube.log_view.as_ref().unwrap().show_source);
        kube.log_button("log-end").unwrap();
        assert_eq!(host.view_messages(), [("kube".to_owned(), "end".to_owned())]);
        kube.log_button("log-clear").unwrap();
        assert!(kube.log_view.as_ref().unwrap().lines.is_empty());
    }

    #[test]
    fn download_suggests_a_path_inside_the_project_and_never_in_tmp() {
        assert_eq!(suggested_path(Some("/work/app/"), "web.log").as_deref(), Some("/work/app/web.log"));
        assert_eq!(suggested_path(None, "web.log"), None);
    }

    #[test]
    fn download_writes_the_shown_lines_with_tee_and_without_it_only_notifies() {
        let (host, _) = host(PREFIXED);
        let host = host.with_capabilities(&["process:kubectl", "process:tee", "storage"]);
        let written: std::rc::Rc<std::cell::RefCell<Vec<(Vec<String>, String)>>> = Default::default();
        let seen = written.clone();
        host.on_process("tee", move |args, stdin| {
            seen.borrow_mut().push((args.to_vec(), stdin.unwrap_or_default().to_owned()));
            Ok(ProcessOutput { status: Some(0), stdout: String::new(), stderr: String::new() })
        });
        let mut kube = KubeExplorer::default();
        kube.open_log_view("pods", "web").unwrap();
        kube.log_view.as_mut().unwrap().filter = "two".into();
        host.answer_next_prompt(Some(&["/tmp/web.log"]));
        kube.log_button("log-download").unwrap();
        assert_eq!(written.borrow()[0].0, ["/tmp/web.log"]);
        assert_eq!(written.borrow()[0].1, "[pod/web/side] 2025-11-06T10:00:00Z two\n");

        let denied = MockHost::install().with_capabilities(&["process:kubectl", "storage"]);
        denied.answer_next_prompt(Some(&["/tmp/web.log"]));
        kube.log_button("log-download").unwrap();
        assert!(denied.notifications()[0].body.contains("tee"));
    }

    #[test]
    fn the_view_renders_as_html_with_the_filter_buttons() {
        let (_host, _) = host(LOG);
        let mut kube = KubeExplorer::default();
        kube.open_log_view("pods", "web").unwrap();
        let rendered = kube.render_tree().unwrap();
        assert!(matches!(rendered.body, Body::Html(_)));
        assert!(rendered.toolbar.iter().any(|b| b.id == "log-copy"));
    }
}
