//! A block scanner for `.gitlab-ci.yml`: top-level keys, `stages`, `include` and, per job, `needs`,
//! `dependencies`, `extends`, `stage` and the script keys. It is not a YAML parser; shapes it does not
//! recognise yield no findings. Written from the GitLab CI/CD YAML reference.

use std::collections::HashSet;
use std::ops::Range;

use openidea_plugin::Severity;

use crate::ci_data;

const RESERVED: &[&str] = &[
    "stages", "variables", "default", "include", "workflow", "image", "services", "before_script", "after_script",
    "cache", "types", "<<",
];
const DEFAULT_STAGES: [&str; 5] = [".pre", "build", "test", "deploy", ".post"];

#[derive(Clone, Debug, PartialEq)]
pub struct Ref {
    pub name: String,
    pub range: Range<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub start: usize,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Job {
    pub name: String,
    pub name_range: Range<usize>,
    pub needs: Vec<Ref>,
    pub dependencies: Vec<Ref>,
    pub extends: Vec<Ref>,
    pub stage: Option<Ref>,
    pub script: Vec<Line>,
}

#[derive(Clone, Debug, Default)]
pub struct CiFile {
    pub jobs: Vec<Job>,
    pub stages: Option<Vec<Ref>>,
    /// `stages:` is present but not fully read (alias, tag, multi-line flow list): its contents are unknown.
    pub stages_unknown: bool,
    /// An `include:` that is not read: remote, project, template, component, a glob, or a local file that failed to read.
    pub has_include: bool,
    pub local_includes: Vec<String>,
    pub included_jobs: HashSet<String>,
    pub included_stages: Option<Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub start: usize,
    pub end: usize,
    pub code: &'static str,
    pub message: String,
    pub severity: Severity,
}

pub fn is_ci_path(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);

    name.contains(".gitlab-ci.") && (name.ends_with(".yml") || name.ends_with(".yaml"))
}

struct Ln<'a> {
    start: usize,
    indent: usize,
    text: &'a str,
}

struct Item {
    head: Range<usize>,
    tail: Vec<Range<usize>>,
}

fn lines(text: &str) -> Vec<Ln<'_>> {
    let mut out = Vec::new();
    let mut offset = 0;

    for piece in text.split_inclusive('\n') {
        let start = offset;
        offset += piece.len();

        let line = piece.trim_end_matches(['\n', '\r']).trim_end();
        let indent = line.len() - line.trim_start_matches(' ').len();
        let body = &line[indent..];
        if body.is_empty() || body.starts_with(['#', '\t']) || body == "---" || body == "..." {
            continue;
        }

        out.push(Ln { start: start + indent, indent, text: body });
    }

    out
}

fn is_dash(text: &str) -> bool {
    text == "-" || text.starts_with("- ")
}

fn block_end(lines: &[Ln], i: usize) -> usize {
    let mut j = i + 1;
    while j < lines.len() && (lines[j].indent > lines[i].indent || (lines[j].indent == lines[i].indent && is_dash(lines[j].text))) {
        j += 1;
    }

    j
}

fn strip_comment(s: &str) -> &str {
    let mut quote = None;
    let mut previous = ' ';

    for (i, c) in s.char_indices() {
        match (quote, c) {
            (None, '\'' | '"') => quote = Some(c),
            (Some(q), _) if q == c => quote = None,
            (None, '#') if previous.is_whitespace() => return s[..i].trim_end(),
            _ => {}
        }
        previous = c;
    }

    s
}

/// The key's range in `body` and where the value starts.
fn split_key(body: &str) -> Option<(Range<usize>, usize)> {
    let first = body.chars().next()?;
    if is_dash(body) || matches!(first, '[' | '{' | '*' | '&' | '?' | '!' | '|' | '>') {
        return None;
    }

    if matches!(first, '"' | '\'') {
        let close = 1 + body[1..].find(first)?;
        if !body[close + 1..].starts_with(':') {
            return None;
        }

        return Some((1..close, close + 2));
    }

    let mut chars = body.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == ':' && chars.peek().is_none_or(|(_, next)| next.is_whitespace()) {
            return (i > 0).then(|| (0..body[..i].trim_end().len(), i + 1));
        }
    }

    None
}

fn trim(text: &str, r: Range<usize>) -> Range<usize> {
    let s = &text[r.clone()];
    let start = r.start + (s.len() - s.trim_start().len());
    let end = r.start + s.trim_end().len();

    match start < end {
        true => start..end,
        false => r.start..r.start,
    }
}

/// Where the text after a key starts, without its trailing comment.
fn value<'a>(l: &Ln<'a>, value_start: usize) -> (usize, &'a str) {
    let s = l.text.get(value_start..).unwrap_or("");
    let lead = s.len() - s.trim_start().len();

    (l.start + value_start.min(l.text.len()) + lead, strip_comment(s.trim_start()))
}

fn split_commas(text: &str, r: Range<usize>) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quote = None;
    let mut from = r.start;

    for (i, c) in text[r.clone()].char_indices() {
        match (quote, c) {
            (None, '\'' | '"') => quote = Some(c),
            (Some(q), _) if q == c => quote = None,
            (None, '[' | '{') => depth += 1,
            (None, ']' | '}') => depth -= 1,
            (None, ',') if depth == 0 => {
                out.push(trim(text, from..r.start + i));
                from = r.start + i + 1;
            }
            _ => {}
        }
    }
    out.push(trim(text, from..r.end));

    out.into_iter().filter(|r| !r.is_empty()).collect()
}

fn scalar_ref(text: &str, r: Range<usize>) -> Option<Ref> {
    let mut r = trim(text, r);
    let s = &text[r.clone()];
    let quoted = s.len() >= 2 && matches!(s.chars().next(), Some('"' | '\'')) && s.ends_with(&s[..1]);
    if quoted {
        r = r.start + 1..r.end - 1;
    }

    let name = &text[r.clone()];
    let dynamic = name.is_empty()
        || name.starts_with(['$', '*', '!', '&', '|', '>', '[', '{', '@', '%'])
        || name.contains(['$', '{'])
        || name.contains(": ");
    if dynamic {
        return None;
    }

    Some(Ref { name: name.to_owned(), range: r })
}

fn block_items(sub: &[Ln]) -> Vec<Item> {
    let Some(first) = sub.iter().find(|l| is_dash(l.text)) else { return Vec::new() };
    let dash_indent = first.indent;
    let mut items: Vec<Item> = Vec::new();

    for l in sub {
        if l.indent == dash_indent && is_dash(l.text) {
            let after = &l.text[1..];
            let start = l.start + 1 + (after.len() - after.trim_start().len());
            let head = strip_comment(after.trim_start());
            items.push(Item { head: start..start + head.len(), tail: Vec::new() });
        } else if l.indent > dash_indent {
            if let Some(item) = items.last_mut() {
                item.tail.push(l.start..l.start + strip_comment(l.text).len());
            }
        }
    }

    items
}

fn items(text: &str, sub: &[Ln], rest: (usize, &str)) -> Vec<Item> {
    let (start, rest) = rest;
    if rest.is_empty() {
        return block_items(sub);
    }

    if !rest.starts_with('[') {
        return vec![Item { head: start..start + rest.len(), tail: Vec::new() }];
    }

    if !rest.ends_with(']') {
        return Vec::new();
    }

    split_commas(text, start + 1..start + rest.len() - 1)
        .into_iter()
        .map(|head| Item { head, tail: Vec::new() })
        .collect()
}

fn pairs(text: &str, segments: &[Range<usize>]) -> Vec<(String, Range<usize>)> {
    segments
        .iter()
        .filter_map(|seg| {
            let s = &text[seg.clone()];
            let (key, value_start) = split_key(s)?;
            Some((s[key].to_owned(), trim(text, seg.start + value_start..seg.end)))
        })
        .collect()
}

fn needs_ref(text: &str, item: &Item) -> Option<Ref> {
    let head = &text[item.head.clone()];
    let pairs = if head.starts_with('{') {
        if !head.ends_with('}') {
            return None;
        }
        pairs(text, &split_commas(text, item.head.start + 1..item.head.end - 1))
    } else if split_key(head).is_some() {
        let mut segments = vec![item.head.clone()];
        segments.extend(item.tail.iter().cloned());
        pairs(text, &segments)
    } else {
        return scalar_ref(text, item.head.clone());
    };

    if pairs.iter().any(|(k, _)| matches!(k.as_str(), "project" | "pipeline" | "trigger")) {
        return None;
    }

    let (_, range) = pairs.into_iter().find(|(k, _)| k == "job")?;
    scalar_ref(text, range)
}

fn refs(text: &str, items: Vec<Item>, mapping: bool) -> Vec<Ref> {
    items
        .iter()
        .filter_map(|item| match mapping {
            true => needs_ref(text, item),
            false => scalar_ref(text, item.head.clone()),
        })
        .collect()
}

fn script_lines(text: &str, sub: &[Ln], rest: (usize, &str)) -> Vec<Line> {
    let line = |r: Range<usize>| Line { start: r.start, text: text[r].to_owned() };
    let (start, rest) = rest;
    if rest.starts_with(['|', '>']) {
        return sub.iter().map(|l| line(l.start..l.start + strip_comment(l.text).len())).collect();
    }

    items(text, sub, (start, rest))
        .into_iter()
        .flat_map(|item| match text[item.head.clone()].starts_with(['|', '>']) {
            true => item.tail,
            false => vec![item.head],
        })
        .map(line)
        .collect()
}

fn parse_job(text: &str, name: &str, name_range: Range<usize>, sub: &[Ln]) -> Job {
    let mut job = Job { name: name.to_owned(), name_range, ..Job::default() };
    let Some(child_indent) = sub.first().map(|l| l.indent) else { return job };

    let mut k = 0;
    while k < sub.len() {
        let end = block_end(sub, k);
        if sub[k].indent == child_indent {
            if let Some((key, value_start)) = split_key(sub[k].text) {
                let rest = value(&sub[k], value_start);
                let body = &sub[k + 1..end];

                match &sub[k].text[key] {
                    "needs" => job.needs = refs(text, items(text, body, rest), true),
                    "dependencies" => job.dependencies = refs(text, items(text, body, rest), false),
                    "extends" => job.extends = refs(text, items(text, body, rest), false),
                    "stage" => job.stage = scalar_ref(text, rest.0..rest.0 + rest.1.len()),
                    "script" | "before_script" | "after_script" => job.script.extend(script_lines(text, body, rest)),
                    _ => {}
                }
            }
        }
        k = end;
    }

    job
}

pub fn parse(text: &str) -> CiFile {
    let ls = lines(text);
    let mut file = CiFile::default();
    let mut i = 0;

    while i < ls.len() {
        let end = block_end(&ls, i);
        if ls[i].indent == 0 {
            if let Some((key, value_start)) = split_key(ls[i].text) {
                let name = &ls[i].text[key.clone()];
                let rest = value(&ls[i], value_start);
                let body = &ls[i + 1..end];

                match name {
                    "stages" => {
                        let listed = items(text, body, rest);
                        let count = listed.len();
                        let stages = refs(text, listed, false);
                        file.stages_unknown = count == 0 || stages.len() != count;
                        file.stages = Some(stages);
                    }
                    "include" => read_includes(text, &mut file, items(text, body, rest)),
                    _ if RESERVED.contains(&name) => {}
                    _ => {
                        let range = ls[i].start + key.start..ls[i].start + key.end;
                        let block = rest.1.is_empty() || rest.1.starts_with(['&', '*']);
                        file.jobs.push(parse_job(text, name, range, if block { body } else { &[] }));
                    }
                }
            }
        }
        i = end;
    }

    file
}

fn read_includes(text: &str, file: &mut CiFile, entries: Vec<Item>) {
    if entries.is_empty() {
        file.has_include = true;
    }

    for item in &entries {
        let head = &text[item.head.clone()];
        let keyed = head.starts_with('{') || split_key(head).is_some();
        let target = match keyed {
            false => scalar_ref(text, item.head.clone()).filter(|r| !r.name.starts_with("http")),
            true => {
                let segments = match head.starts_with('{') && head.ends_with('}') {
                    true => split_commas(text, item.head.start + 1..item.head.end - 1),
                    false => std::iter::once(item.head.clone()).chain(item.tail.iter().cloned()).collect(),
                };
                let (_, range) = pairs(text, &segments).into_iter().find(|(k, _)| k == "local").unzip();
                range.and_then(|r| scalar_ref(text, r))
            }
        };

        match target.filter(|r| !r.name.contains('*')) {
            Some(local) => file.local_includes.push(local.name),
            None => file.has_include = true,
        }
    }
}

const MAX_INCLUDE_DEPTH: usize = 3;

/// Reads each local include through `read` (given the path as written in the file) and adds its jobs and
/// stages. Anything unreadable, or nested deeper than three files, counts as an unread include.
pub fn resolve_includes(file: &mut CiFile, read: &dyn Fn(&str) -> Option<String>) {
    let mut seen = HashSet::new();
    let paths = std::mem::take(&mut file.local_includes);
    let mut queue: Vec<(String, usize)> = paths.into_iter().map(|p| (p, 1)).collect();

    while let Some((path, depth)) = queue.pop() {
        if !seen.insert(path.clone()) {
            continue;
        }

        let Some(included) = read(&path).filter(|_| depth <= MAX_INCLUDE_DEPTH).map(|t| parse(&t)) else {
            file.has_include = true;
            continue;
        };

        file.included_jobs.extend(included.jobs.iter().map(|j| j.name.clone()));
        file.has_include |= included.has_include || included.stages_unknown;
        if let Some(stages) = included.stages {
            let names = stages.into_iter().map(|s| s.name);
            file.included_stages.get_or_insert_with(Vec::new).extend(names);
        }
        queue.extend(included.local_includes.into_iter().map(|p| (p, depth + 1)));
    }
}

fn finding(range: &Range<usize>, code: &'static str, message: String, severity: Severity) -> Finding {
    Finding { start: range.start, end: range.end, code, message, severity }
}

pub fn check(file: &CiFile) -> Vec<Finding> {
    let mut out = Vec::new();
    let defined: HashSet<&str> = file.jobs.iter().map(|j| j.name.as_str()).chain(file.included_jobs.iter().map(String::as_str)).collect();
    let unread = file.has_include || !file.local_includes.is_empty();
    let stages: Option<HashSet<&str>> = match &file.stages {
        Some(_) if file.stages_unknown => None,
        Some(stages) => Some(stages.iter().map(|s| s.name.as_str()).collect()),
        None if unread => None,
        None => match &file.included_stages {
            Some(listed) => Some(listed.iter().map(String::as_str).collect()),
            None => Some(DEFAULT_STAGES.into()),
        },
    };

    for job in &file.jobs {
        for (list, keyword) in [(&job.needs, "needs"), (&job.dependencies, "dependencies"), (&job.extends, "extends")] {
            let mut seen = HashSet::new();
            for entry in list {
                if !seen.insert(entry.name.as_str()) {
                    let message = format!("`{}` is already listed in `{keyword}`", entry.name);
                    out.push(finding(&entry.range, "duplicate-job-usage", message, Severity::Warning));
                }

                if !unread && !defined.contains(entry.name.as_str()) {
                    let message = format!("`{keyword}` names `{}`, which is not a job in this file", entry.name);
                    out.push(finding(&entry.range, "undefined-job", message, Severity::Warning));
                }
            }
        }

        if let (Some(stage), Some(known)) = (&job.stage, &stages) {
            if !known.contains(stage.name.as_str()) {
                let message = format!("Stage `{}` is not listed in `stages`", stage.name);
                out.push(finding(&stage.range, "undefined-stage", message, Severity::Warning));
            }
        }

        for line in &job.script {
            if let Some(range) = single_quoted_variable(&line.text) {
                let range = line.start + range.start..line.start + range.end;
                let message = "The shell does not expand variables inside single quotes; use double quotes".to_owned();
                out.push(finding(&range, "single-quoted-shell-variable", message, Severity::Info));
            }
        }
    }

    out
}

/// Only an `echo`/`printf` whose sole argument is one single-quoted string: anything else (`sh -c '…'`,
/// `sed 's/$A/…/'`) may use the quotes on purpose.
fn single_quoted_variable(line: &str) -> Option<Range<usize>> {
    let mut from = 0;
    let mut to = line.len();
    if line.starts_with('"') {
        if to < 2 || !line.ends_with('"') || line.contains('\\') {
            return None;
        }
        from = 1;
        to -= 1;
    }

    let command = &line[from..to];
    let rest = command.strip_prefix("echo ").or_else(|| command.strip_prefix("printf "))?;
    let mut argument = from + (command.len() - rest.len());
    while line[argument..to].starts_with('-') {
        let flag = line[argument..to].find(' ')?;
        argument += flag + 1;
        argument += line[argument..to].len() - line[argument..to].trim_start().len();
    }

    let quoted = &line[argument..to];
    let body = quoted.strip_prefix('\'')?.strip_suffix('\'')?;
    if body.contains('\'') {
        return None;
    }

    let body_start = argument + 1;
    let bytes = body.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'$' {
            continue;
        }

        let name = &body[i + 1..];
        if name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
            let len = name.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(name.len());
            return Some(body_start + i..body_start + i + 1 + len);
        }

        if name.starts_with('{') {
            let close = name.find('}')?;
            return Some(body_start + i..body_start + i + close + 2);
        }
    }

    None
}

/// The range to open for the job or stage named at byte `offset`.
pub fn target_at(file: &CiFile, offset: usize) -> Option<Range<usize>> {
    let contains = |r: &Ref| r.range.start <= offset && offset <= r.range.end;

    for job in &file.jobs {
        let referenced = job.needs.iter().chain(&job.dependencies).chain(&job.extends).find(|r| contains(r));
        if let Some(entry) = referenced {
            return file.jobs.iter().find(|j| j.name == entry.name).map(|j| j.name_range.clone());
        }

        if let Some(stage) = job.stage.as_ref().filter(|s| contains(s)) {
            return file.stages.iter().flatten().find(|s| s.name == stage.name).map(|s| s.range.clone());
        }
    }

    None
}

pub struct Candidate {
    pub label: String,
    pub insert: String,
    /// Byte offset where the typed prefix starts.
    pub from: usize,
    pub variable: bool,
}

/// Predefined variables after `$` or `${`, else keywords for the key being typed: globals at the top level,
/// job keywords one level inside a job or `default`.
pub fn completions(text: &str, offset: Option<usize>) -> Vec<Candidate> {
    let Some(offset) = offset.filter(|o| text.is_char_boundary(*o)) else { return Vec::new() };
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let before = &text[line_start..offset];
    let typed = before.trim_start_matches(' ');
    let word = |s: &str| s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');

    let word_in_line = before.rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).map_or(0, |i| i + 1);
    let word_start = line_start + word_in_line;
    let after_dollar = before[..word_in_line].trim_end_matches('{').ends_with('$');
    if after_dollar {
        let prefix = &text[word_start..offset];
        return ci_data::CI_VARIABLES
            .iter()
            .filter(|name| name.starts_with(prefix))
            .map(|name| Candidate { label: (*name).into(), insert: (*name).into(), from: word_start, variable: true })
            .collect();
    }

    if !word(typed) {
        return Vec::new();
    }

    let indent = before.len() - typed.len();
    let keywords = match indent {
        0 => ci_data::GLOBAL_KEYWORDS,
        _ if parent_is_job(&text[..line_start], indent) => ci_data::JOB_KEYWORDS,
        _ => return Vec::new(),
    };
    let from = line_start + indent;

    keywords
        .iter()
        .filter(|name| name.starts_with(typed))
        .map(|name| Candidate { label: (*name).into(), insert: format!("{name}:"), from, variable: false })
        .collect()
}

fn parent_is_job(above: &str, indent: usize) -> bool {
    let parent = above
        .lines()
        .rev()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .find(|l| l.len() - l.trim_start_matches(' ').len() < indent);
    let Some(parent) = parent else { return false };
    let name = parent.trim_end().strip_suffix(':').unwrap_or("");
    let top_level = !parent.starts_with(' ') && !name.is_empty() && !name.contains(' ');

    top_level && (name == "default" || !RESERVED.contains(&name))
}
