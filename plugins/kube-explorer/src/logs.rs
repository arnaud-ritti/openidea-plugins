//! A log snapshot in the panel: one `kubectl logs --tail=N --timestamps` run,
//! filtered in the plugin by text or regular expression, case and start time,
//! with soft wrap and a copy of the shown lines. Following a log stays a
//! terminal command: a view cannot stream and a plugin cannot write a file.

use openidea_plugin::views::{Body, Button, View};
use openidea_plugin::{Error, NotificationLevel, host};
use regex_lite::{Regex, RegexBuilder, escape};

use crate::{KubeExplorer, REQUEST_TIMEOUT, WORKLOADS, escape_html};

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
    wrap: bool,
}

struct Shown<'a> {
    lines: Vec<&'a str>,
    matcher: Option<Regex>,
    problem: Option<String>,
}

fn timestamp(line: &str) -> Option<&str> {
    let token = line.split(' ').next()?;
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
        let mut after_since = since.is_empty();
        let mut lines = Vec::new();
        for line in &self.lines {
            if let (Some(stamp), false) = (timestamp(line), since.is_empty()) {
                let length = since.len().min(stamp.len());
                after_since = stamp[..length] >= since[..length];
            }
            if after_since && matcher.as_ref().is_none_or(|m| m.is_match(line)) {
                lines.push(line.as_str());
            }
        }
        if lines.len() > MAX_SHOWN_LINES {
            lines.drain(..lines.len() - MAX_SHOWN_LINES);
        }
        Shown { lines, matcher, problem }
    }

    fn page(&self, shown: &Shown) -> String {
        let body: Vec<String> = shown
            .lines
            .iter()
            .map(|line| match &shown.matcher {
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
            })
            .collect();
        let white_space = if self.wrap { "pre-wrap" } else { "pre" };
        format!(
            "<style>:root{{--bg:Canvas;--fg:CanvasText}}body{{margin:0;background:var(--openidea-bg,var(--bg));color:var(--openidea-fg,var(--fg))}}\
             pre{{margin:0;padding:8px 10px;font:12px/1.5 var(--openidea-font-mono,ui-monospace,Menlo,monospace);white-space:{white_space};word-break:break-all}}\
             mark{{background:color-mix(in srgb,var(--openidea-warning,#d6ae58) 55%,transparent);color:inherit}}</style><pre>{}</pre>",
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
            .button(Button::new("log-regex", &toggle("Regex", self.regex)))
            .button(Button::new("log-case", &toggle("Match case", self.match_case)))
            .button(Button::new("log-wrap", &toggle("Wrap", self.wrap)))
            .button(Button::new("log-refresh", "Refresh").icon("refresh"))
            .button(Button::new("log-copy", "Copy").icon("copy"))
    }
}

impl KubeExplorer {
    fn fetch_log_lines(&self, kind: &str, row: &str) -> Result<(Vec<String>, bool), Error> {
        let (namespace, name) = self.split_scope(kind, row)?;
        let tail = host::setting_number("logViewerLines").unwrap_or(DEFAULT_TAIL).clamp(10.0, 20000.0) as u32;
        let tail = format!("--tail={tail}");
        let target = format!("{kind}/{name}");
        let args: Vec<&str> = match kind {
            "pods" => vec!["logs", &name, "--timestamps", "--all-containers", &tail, REQUEST_TIMEOUT],
            _ => vec!["logs", &target, "--all-pods=true", "--all-containers", "--max-log-requests=20", "--timestamps", &tail, REQUEST_TIMEOUT],
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
            wrap: previous.as_ref().is_none_or(|v| v.wrap),
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
            "log-regex" => view.regex = !view.regex,
            "log-case" => view.match_case = !view.match_case,
            "log-wrap" => view.wrap = !view.wrap,
            "log-copy" => {
                let shown = view.shown();
                let text = shown.lines.join("\n");
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
            wrap: true,
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
        assert_eq!(runs.borrow()[0], ["--namespace=shop", "logs", "web", "--timestamps", "--all-containers", "--tail=50", REQUEST_TIMEOUT]);

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
