//! Chart-aware template checks: `.Values.a.b` paths against the chart's
//! `values.yaml` and `include` names against the `define`s of its templates.
//! Text only, so everything dynamic keeps it quiet: `tpl`, a rebound `.`,
//! subcharts, aliases in `values.yaml`.

use std::collections::{BTreeMap, BTreeSet};

use openidea_plugin::Severity;
use openidea_plugin::Project;

use crate::checks::Finding;

pub const HELM_UNKNOWN_VALUE: &str = "helm-unknown-value";
pub const HELM_UNKNOWN_INCLUDE: &str = "helm-unknown-include";

const MAX_TEMPLATES: u32 = 500;
const MAX_TEMPLATE_BYTES: usize = 1 << 20;
const GUARDS: &[&str] = &["default", "required", "hasKey", "coalesce", "ternary", "get", "dig", "pluck", "kindIs", "empty"];
const REBINDING: &[&str] = &["with", "range", "define", "block"];
const CONTROLS: &[&str] = &["if", "else", "with", "range", "and", "or", "not"];

#[derive(Debug, PartialEq, Eq)]
pub enum Node {
    Open,
    Map(BTreeMap<String, Node>),
}

#[derive(Default)]
pub struct Chart {
    pub values: Option<Node>,
    pub defines: Option<BTreeSet<String>>,
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn unquote(key: &str) -> &str {
    key.trim_matches(|c| c == '"' || c == '\'')
}

fn node_at<'a>(root: &'a mut BTreeMap<String, Node>, path: &[String]) -> &'a mut Node {
    let (first, rest) = path.split_first().expect("a path has a key");
    let node = root.get_mut(first).expect("parents are inserted first");
    match rest.is_empty() {
        true => node,
        false => match node {
            Node::Map(children) => node_at(children, rest),
            Node::Open => unreachable!("only maps are descended"),
        },
    }
}

/// `None` when the file uses aliases or merge keys, which this scan cannot follow.
pub fn parse_values(text: &str) -> Option<Node> {
    if text.contains("<<:") || text.lines().any(|line| line.contains(": *") || line.contains(": &") || line.trim_start().starts_with("- *")) {
        return None;
    }
    let mut root = BTreeMap::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut block_scalar: Option<usize> = None;
    let mut list_at: Option<usize> = None;
    for line in text.lines() {
        let content = line.trim();
        if content.is_empty() || content.starts_with('#') || content == "---" {
            continue;
        }
        let indent = indent_of(line);
        if block_scalar.is_some_and(|at| indent > at) {
            continue;
        }
        block_scalar = None;
        if let Some(at) = list_at {
            if indent > at || (indent == at && content.starts_with('-')) {
                continue;
            }
            list_at = None;
        }
        if content.starts_with('-') {
            list_at = Some(indent);
            continue;
        }
        let Some((key, value)) = content.split_once(':') else { continue };
        if key.contains(' ') && !key.starts_with(['"', '\'']) {
            continue;
        }
        if !value.is_empty() && !value.starts_with(' ') {
            continue;
        }
        while stack.last().is_some_and(|(at, _)| *at >= indent) {
            stack.pop();
        }
        let path: Vec<String> = stack.iter().map(|(_, key)| key.clone()).collect();
        let siblings = match path.is_empty() {
            true => &mut root,
            false => {
                let parent = node_at(&mut root, &path);
                if *parent == Node::Open {
                    *parent = Node::Map(BTreeMap::new());
                }
                match parent {
                    Node::Map(children) => children,
                    Node::Open => unreachable!(),
                }
            }
        };
        let key = unquote(key).to_owned();
        siblings.insert(key.clone(), Node::Open);
        let value = value.trim();
        if value.starts_with(['|', '>']) {
            block_scalar = Some(indent);
        }
        if value.is_empty() || value.starts_with('#') {
            stack.push((indent, key));
        }
    }
    Some(Node::Map(root))
}

struct Action<'a> {
    start: usize,
    text: &'a str,
}

fn actions(text: &str) -> Vec<Action<'_>> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find("{{").map(|i| i + from) {
        let Some(close) = text[open..].find("}}").map(|i| i + open) else { break };
        from = close + 2;
        let inner = &text[open + 2..close];
        if inner.trim_start_matches(['-', ' ']).starts_with("/*") {
            continue;
        }
        found.push(Action { start: open + 2, text: inner });
    }
    found
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|word| !word.is_empty())
}

fn first_word(action: &str) -> &str {
    words(action).next().unwrap_or("")
}

fn blank_strings(action: &str) -> String {
    let mut out = String::with_capacity(action.len());
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in action.chars() {
        match quote {
            Some(q) => {
                let closes = c == q && !escaped;
                escaped = c == '\\' && q == '"' && !escaped;
                out.extend(std::iter::repeat_n(' ', c.len_utf8()));
                if closes {
                    quote = None;
                }
            }
            None => {
                if matches!(c, '"' | '`') {
                    quote = Some(c);
                    out.push(' ');
                } else {
                    out.push(c);
                }
            }
        }
    }
    out
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The segments after `.Values` at every occurrence in a blanked action, with their byte ranges.
fn value_paths(action: &str, root: &str) -> Vec<Vec<(String, usize, usize)>> {
    let needle = format!("{root}.Values");
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = action[from..].find(&needle).map(|i| i + from) {
        from = at + needle.len();
        let before = action[..at].chars().next_back();
        let valid_start = match root {
            "$" => before.is_none_or(|c| !is_ident(c)),
            _ => before.is_none_or(|c| !is_ident(c) && !matches!(c, '.' | ')' | '$')),
        };
        if !valid_start || action[from..].chars().next().is_some_and(is_ident) {
            continue;
        }
        let mut segments = Vec::new();
        let mut cursor = from;
        while let Some(rest) = action[cursor..].strip_prefix('.') {
            let len = rest.find(|c: char| !is_ident(c)).unwrap_or(rest.len());
            if len == 0 {
                break;
            }
            segments.push((rest[..len].to_owned(), cursor + 1, cursor + 1 + len));
            cursor += 1 + len;
        }
        found.push(segments);
    }
    found
}

fn first_missing<'a>(values: &Node, segments: &'a [(String, usize, usize)]) -> Option<(usize, &'a (String, usize, usize))> {
    let mut node = values;
    for (index, segment) in segments.iter().enumerate() {
        if index == 0 && segment.0 == "global" {
            return None;
        }
        let Node::Map(children) = node else { return None };
        match children.get(&segment.0) {
            Some(child) => node = child,
            None => return Some((index, segment)),
        }
    }
    None
}

pub fn check(text: &str, chart: &Chart) -> Vec<Finding> {
    if !text.contains("{{") {
        return Vec::new();
    }
    let mut found = Vec::new();
    let all = actions(text);
    let uses_tpl = all.iter().any(|action| words(&blank_strings(action.text)).any(|word| word == "tpl"));
    let mut blocks: Vec<String> = Vec::new();
    for action in &all {
        let blanked = blank_strings(action.text);
        let head = first_word(&blanked);
        if let (Some(values), false) = (&chart.values, uses_tpl) {
            let dot_is_root = !blocks.iter().any(|b| REBINDING.contains(&b.as_str()));
            let in_define = blocks.iter().any(|b| b == "define" || b == "block");
            let lenient = GUARDS.iter().chain(CONTROLS).any(|guard| words(&blanked).any(|word| word == *guard));
            let roots: &[&str] = match (dot_is_root, in_define) {
                (true, true) => &[],
                (true, false) => &["", "$"],
                (false, true) => &[],
                (false, false) => &["$"],
            };
            for root in roots {
                for segments in value_paths(&blanked, root) {
                    let Some((index, (name, start, end))) = first_missing(values, &segments) else { continue };
                    if index + 1 == segments.len() && lenient {
                        continue;
                    }
                    let path: Vec<&str> = segments[..=index].iter().map(|(s, _, _)| s.as_str()).collect();
                    found.push(Finding {
                        severity: Severity::Info,
                        ..Finding::new(
                            action.start + start,
                            action.start + end,
                            HELM_UNKNOWN_VALUE,
                            format!("`{name}` is not defined in values.yaml (`.Values.{}`); set a default there or guard it.", path.join(".")),
                        )
                    });
                }
            }
        }
        if let Some(defines) = &chart.defines {
            found.extend(unknown_includes(action, defines));
        }
        match head {
            "if" | "with" | "range" | "define" | "block" => blocks.push(head.to_owned()),
            "end" => {
                blocks.pop();
            }
            "else" => {
                let next = words(&blanked).nth(1).unwrap_or("");
                if let (true, Some(top)) = (REBINDING.contains(&next), blocks.last_mut()) {
                    *top = next.to_owned();
                }
            }
            _ => {}
        }
    }
    found
}

fn literal_after<'a>(text: &'a str, word: &str) -> Option<(usize, &'a str)> {
    let mut from = 0;
    while let Some(at) = text[from..].find(word).map(|i| i + from) {
        from = at + word.len();
        let before = text[..at].chars().next_back();
        if before.is_some_and(|c| is_ident(c) || c == '.') {
            continue;
        }
        let rest = &text[from..];
        let spaces = rest.len() - rest.trim_start().len();
        if spaces == 0 {
            continue;
        }
        let rest = &rest[spaces..];
        let Some(quote) = rest.chars().next().filter(|c| matches!(c, '"' | '`')) else { continue };
        let inner = &rest[1..];
        let len = inner.find(quote)?;
        return Some((from + spaces + 1, &inner[..len]));
    }
    None
}

fn unknown_includes(action: &Action, defines: &BTreeSet<String>) -> Vec<Finding> {
    ["include", "template"]
        .iter()
        .filter_map(|word| literal_after(action.text, word))
        .filter(|(_, name)| !name.is_empty() && !defines.contains(*name))
        .map(|(at, name)| {
            Finding::new(
                action.start + at,
                action.start + at + name.len(),
                HELM_UNKNOWN_INCLUDE,
                format!("No `define \"{name}\"` in this chart's templates."),
            )
        })
        .collect()
}

pub fn defined_names(text: &str) -> impl Iterator<Item = String> + '_ {
    actions(text).into_iter().filter_map(|action| {
        let head = first_word(&action.text.trim_start_matches(['-', ' ']).to_owned()).to_owned();
        let word = ["define", "block"].into_iter().find(|w| *w == head)?;
        let after = action.text.find(word)? + word.len();
        let rest = action.text[after..].trim_start();
        let quote = rest.chars().next().filter(|c| matches!(c, '"' | '`'))?;
        let inner = &rest[1..];
        Some(inner[..inner.find(quote)?].to_owned())
    })
}

impl Chart {
    /// `None`: nothing to judge against (no values file, subcharts, unreadable templates).
    pub fn load(project: &Project, root: &str, chart_dir: &str, current: &str) -> Chart {
        let Ok(chart_yaml) = project.read_file(&format!("{chart_dir}/Chart.yaml")) else { return Chart::default() };
        let has_dependencies = chart_yaml.lines().any(|line| line.starts_with("dependencies:") || line.starts_with("requirements:"));
        let relative = chart_dir.strip_prefix(root).unwrap_or(chart_dir).trim_start_matches('/');
        let prefix = match relative.is_empty() {
            true => String::new(),
            false => format!("{relative}/"),
        };
        let has_subcharts = project.find_files(&format!("{prefix}charts/**"), 1).map_or(true, |files| !files.is_empty());
        if has_dependencies || has_subcharts {
            return Chart::default();
        }
        let values = project.read_file(&format!("{chart_dir}/values.yaml")).ok().and_then(|text| parse_values(&text));
        let defines = project
            .find_files(&format!("{prefix}templates/**"), MAX_TEMPLATES)
            .ok()
            .filter(|files| files.len() < MAX_TEMPLATES as usize)
            .map(|files| {
                let mut names: BTreeSet<String> = defined_names(current).collect();
                for file in files {
                    let Ok(text) = project.read_file(&file) else { continue };
                    if text.len() <= MAX_TEMPLATE_BYTES {
                        names.extend(defined_names(&text));
                    }
                }
                names
            });
        Chart { values, defines }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chart(values: &str, defines: &[&str]) -> Chart {
        Chart {
            values: parse_values(values),
            defines: Some(defines.iter().map(|d| (*d).to_owned()).collect()),
        }
    }

    const VALUES: &str = "image:\n  repository: nginx\n  tag: \"1\"\nreplicas: 2\nresources: {}\nhosts:\n  - host: a\n    paths: x\nscript: |\n  foo: bar\nglobal:\n  x: 1\n";

    fn flagged<'a>(text: &'a str, chart: &Chart, code: &str) -> Vec<&'a str> {
        check(text, chart).iter().filter(|f| f.code == code).map(|f| &text[f.start..f.end]).collect()
    }

    #[test]
    fn parses_nested_keys_and_leaves_lists_flow_and_block_scalars_open() {
        let Some(Node::Map(root)) = parse_values(VALUES) else { panic!() };
        assert_eq!(root.keys().collect::<Vec<_>>(), ["global", "hosts", "image", "replicas", "resources", "script"]);
        assert!(matches!(root["image"], Node::Map(_)));
        assert_eq!(root["hosts"], Node::Open);
        assert_eq!(root["resources"], Node::Open);
        assert_eq!(root["script"], Node::Open);
    }

    #[test]
    fn aliases_and_merge_keys_disable_the_values_check() {
        assert!(parse_values("a: &x\n  b: 1\nc: *x\n").is_none());
        assert!(parse_values("a:\n  <<: *x\n").is_none());
    }

    #[test]
    fn a_missing_segment_is_reported_where_it_starts() {
        let c = chart(VALUES, &[]);
        let text = "a: {{ .Values.image.pullPolicy.name }}\nb: {{ .Values.replicas }}\nc: {{ .Values.image.tag }}\nd: {{ $.Values.nope.x }}\n";
        assert_eq!(flagged(text, &c, HELM_UNKNOWN_VALUE), ["pullPolicy", "nope"]);
    }

    #[test]
    fn known_keys_global_and_open_nodes_are_quiet() {
        let c = chart(VALUES, &[]);
        let text = "{{ .Values.global.anything.deep }} {{ .Values.resources.limits.cpu }} {{ .Values.hosts.x }} {{ .Values.script.y }}";
        assert!(flagged(text, &c, HELM_UNKNOWN_VALUE).is_empty());
    }

    #[test]
    fn a_missing_last_segment_is_quiet_when_guarded_or_defaulted() {
        let c = chart(VALUES, &[]);
        let text = "{{ if .Values.image.digest }}{{ end }}{{ .Values.image.pullPolicy | default \"IfNotPresent\" }}{{ .Values.image.other }}";
        assert_eq!(flagged(text, &c, HELM_UNKNOWN_VALUE), ["other"]);
    }

    #[test]
    fn tpl_rebinding_define_and_strings_keep_it_quiet() {
        let c = chart(VALUES, &[]);
        assert!(flagged("{{ tpl .Values.zzz.a . }}{{ .Values.q.w }}", &c, HELM_UNKNOWN_VALUE).is_empty());
        assert!(flagged("{{ range .items }}{{ .Values.q.w }}{{ end }}", &c, HELM_UNKNOWN_VALUE).is_empty());
        assert!(flagged("{{ with .x }}{{ .Values.q.w }}{{ end }}", &c, HELM_UNKNOWN_VALUE).is_empty());
        assert!(flagged("{{ define \"a\" }}{{ .Values.q.w }}{{ end }}", &c, HELM_UNKNOWN_VALUE).is_empty());
        assert!(flagged("{{ print \".Values.q.w\" }}{{/* .Values.q.w */}}", &c, HELM_UNKNOWN_VALUE).is_empty());
        assert_eq!(flagged("{{ with .x }}{{ end }}{{ .Values.q.w }}", &c, HELM_UNKNOWN_VALUE), ["q"]);
        assert_eq!(flagged("{{ with .Values.q.w }}{{ end }}", &c, HELM_UNKNOWN_VALUE), ["q"]);
        assert_eq!(flagged("{{ with .x }}{{ $.Values.q.w }}{{ end }}", &c, HELM_UNKNOWN_VALUE), ["q"]);
    }

    #[test]
    fn without_values_nothing_is_judged() {
        let c = Chart::default();
        assert!(check("{{ .Values.q.w }}{{ include \"x\" . }}", &c).is_empty());
    }

    #[test]
    fn an_include_without_a_define_is_reported() {
        let c = chart("", &["web.name"]);
        let text = "a: {{ include \"web.name\" . }}\nb: {{ include \"web.nope\" . | nindent 4 }}\nc: {{ template \"gone\" . }}\nd: {{ include (print \"x\") . }}\n";
        assert_eq!(flagged(text, &c, HELM_UNKNOWN_INCLUDE), ["web.nope", "gone"]);
    }

    #[test]
    fn define_and_block_names_are_collected() {
        let text = "{{- define \"a.b\" -}}x{{- end }}{{ block \"c\" . }}{{ end }}{{ include \"z\" . }}";
        assert_eq!(defined_names(text).collect::<Vec<_>>(), ["a.b", "c"]);
    }
}
