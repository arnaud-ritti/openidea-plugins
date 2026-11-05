//! Spring Batch Tools for OpenIDEA: navigation, completion and checks for
//! XML job configuration (steps, flows, transitions, bean references), job
//! names in `spring.batch.job.name`, and Spring Batch 5 migration hints in
//! Java configuration. Text-based: names match as written.

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{Diagnostic, Error, Invocation, Location, Plugin, Position, Range, Severity, host, register_plugin};
use spring_text::index::{Index, Indexes, PROVIDER_BUDGET};
use spring_text::{beans, config, contains, java, range, xml};
use std::collections::BTreeSet;

const NAMESPACE: &str = "http://www.springframework.org/schema/batch";
/// Elements a transition can lead to.
const STATES: [&str; 4] = ["step", "flow", "split", "decision"];
/// Attributes of the batch namespace that name a bean.
const BEAN_ATTRS: [&str; 13] = [
    "ref",
    "reader",
    "writer",
    "processor",
    "skip-policy",
    "retry-policy",
    "decider",
    "job-repository",
    "transaction-manager",
    "task-executor",
    "job-launcher",
    "partitioner",
    "chunk-completion-policy",
];
const JOB_NAME_KEY: &str = "spring.batch.job.name";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `<job id>`, `new JobBuilder("x", …)`, `jobBuilderFactory.get("x")`.
    Job,
    /// A step, flow, split or decision id (or a Java step name).
    State,
    /// A bean of any kind.
    Bean,
    /// A `@SpringBootApplication` class.
    BootApp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    pub name: String,
    pub kind: Kind,
    pub range: Range,
    /// The enclosing job (or top-level flow) element of an XML state.
    pub scope: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    /// `next`, `to`, `restart`: a state of the same job.
    Target,
    /// `parent`: a step, flow or job anywhere.
    Parent,
    Bean,
}

/// A reference written in a batch XML attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub value: String,
    pub kind: LinkKind,
    pub range: Range,
    pub scope: Option<usize>,
}

/// The nearest enclosing `<job>`, else the outermost enclosing `<flow>`.
fn scope_of(els: &[xml::Element], i: usize) -> Option<usize> {
    let mut flow = None;
    let mut at = els[i].parent;
    while let Some(p) = at {
        match els[p].local() {
            "job" => return Some(p),
            "flow" => flow = Some(p),
            _ => {}
        }
        at = els[p].parent;
    }
    flow
}

/// The names and references of a Spring XML file.
pub fn parse_xml(text: &str) -> (Vec<Name>, Vec<Link>) {
    if !text.contains("www.springframework.org/schema/") {
        return Default::default();
    }
    let els = xml::elements(text);
    let attr_range = |a: &xml::Attr| range(text, a.value_at, a.value_end);
    let mut names: Vec<Name> = xml::bean_ids(&els)
        .into_iter()
        .map(|a| Name { name: a.value.clone(), kind: Kind::Bean, range: attr_range(a), scope: None })
        .collect();
    let mut links = Vec::new();
    if !text.contains(NAMESPACE) {
        return (names, links);
    }
    for (i, e) in els.iter().enumerate().filter(|(i, _)| xml::namespace(&els, *i) == Some(NAMESPACE)) {
        let scope = scope_of(&els, i);
        if let Some(id) = e.attr("id") {
            let kind = match e.local() {
                "job" => Some(Kind::Job),
                l if STATES.contains(&l) => Some(Kind::State),
                _ => None,
            };
            if let Some(kind) = kind {
                names.push(Name { name: id.value.clone(), kind, range: attr_range(id), scope });
            }
        }
        for a in &e.attrs {
            let kind = match a.local() {
                "next" | "to" | "restart" => LinkKind::Target,
                "parent" => LinkKind::Parent,
                l if BEAN_ATTRS.contains(&l) => LinkKind::Bean,
                _ => continue,
            };
            links.push(Link { value: a.value.trim().to_owned(), kind, range: attr_range(a), scope });
        }
    }
    (names, links)
}

/// The names a Java or Kotlin source declares.
pub fn parse_source(path: &str, text: &str) -> Vec<Name> {
    let file = spring_text::parse_source(path, text);
    let name = |name: String, kind, from, to| Name { name, kind, range: range(text, from, to), scope: None };
    let mut out: Vec<Name> =
        java::bean_names(&file).into_iter().map(|b| name(b.name, Kind::Bean, b.at, b.at + b.len)).collect();
    out.extend(file.types.iter().filter(|t| t.has(&["SpringBootApplication"])).map(|t| {
        name(t.name.clone(), Kind::BootApp, t.at, t.at + t.name.len())
    }));
    if !text.contains("Builder") {
        return out;
    }
    for c in spring_text::source_calls(path, text) {
        let Some(s) = c.args.first().and_then(|a| a.string.as_ref()) else { continue };
        let receiver = c.receiver.as_deref().unwrap_or_default().to_lowercase();
        let kind = match c.name.as_str() {
            "JobBuilder" if c.is_new => Kind::Job,
            "StepBuilder" if c.is_new => Kind::State,
            "get" if receiver.contains("jobbuilder") => Kind::Job,
            "get" if receiver.contains("stepbuilder") => Kind::State,
            _ => continue,
        };
        out.push(name(s.value.clone(), kind, s.at, s.end));
    }
    out
}

fn index_file(path: &str, text: &str) -> Option<Vec<Name>> {
    let names = if path.ends_with(".xml") { parse_xml(text).0 } else { parse_source(path, text) };
    Some(names).filter(|n| !beans::ignored(path) && !n.is_empty())
}

/// Spring Batch 5 hints for a Java source: deprecated builder factories, and
/// `@EnableBatchProcessing` next to Spring Boot.
pub fn java_problems(text: &str, boot: bool) -> Vec<Diagnostic> {
    let file = java::parse(text);
    let imported = |at: usize| file.imports.iter().any(|i| i.at <= at && at <= i.end);
    let deprecated = |from: usize, to: usize, factory: &str| {
        let builder = factory.trim_end_matches("Factory");
        Diagnostic {
            deprecated: true,
            ..Diagnostic::new(
                range(text, from, to),
                Severity::Warning,
                format!("{factory} is deprecated since Spring Batch 5. Use new {builder}(name, jobRepository) instead."),
            )
            .code("deprecated-builder-factory")
        }
    };
    let mut out: Vec<Diagnostic> = java::tokens(text)
        .into_iter()
        .filter(|t| matches!(t.s, "JobBuilderFactory" | "StepBuilderFactory") && !imported(t.at))
        .map(|t| deprecated(t.at, t.end(), t.s))
        .collect();
    for c in java::calls(text).into_iter().filter(|c| c.name == "get") {
        let Some(receiver) = c.receiver.as_deref() else { continue };
        let lower = receiver.to_lowercase();
        let factory = if lower.contains("jobbuilder") {
            "JobBuilderFactory"
        } else if lower.contains("stepbuilder") {
            "StepBuilderFactory"
        } else {
            continue;
        };
        let written = format!("{receiver}.");
        let from = c.at.checked_sub(written.len()).filter(|&f| text[f..c.at] == written).unwrap_or(c.at);
        out.push(deprecated(from, c.at + c.name.len(), factory));
    }
    if boot {
        let enable = file.types.iter().flat_map(|t| &t.annotations).filter(|a| a.name == "EnableBatchProcessing");
        out.extend(enable.map(|a| {
            Diagnostic::new(
                range(text, a.at, a.end),
                Severity::Info,
                "With Spring Boot 3, @EnableBatchProcessing switches off Boot's batch auto-configuration (running jobs at startup, schema initialisation, spring.batch.* properties). Remove it unless you want that.",
            )
            .code("enable-batch-processing-with-boot")
        }));
    }
    out
}

/// The job names of a `spring.batch.job.name` value (comma-separated): `(name, from, to)` bytes.
fn job_name_items(path: &str, text: &str) -> Vec<(String, usize, usize)> {
    let key = config::canonical(JOB_NAME_KEY);
    let mut out = Vec::new();
    for k in config::keys(path, text).into_iter().filter(|k| config::canonical(&k.key) == key) {
        let mut at = k.value_at;
        for part in text[k.value_at..k.value_end].split(',') {
            let lead = part.len() - part.trim_start().len();
            out.push((part.trim().to_owned(), at + lead, at + lead + part.trim().len()));
            at += part.len() + 1;
        }
    }
    out
}

fn file_kind(doc: &Document) -> Option<&'static str> {
    match doc.language.as_str() {
        "xml" if doc.text.contains(NAMESPACE) => Some("xml"),
        "java" if doc.text.contains("batch") => Some("java"),
        _ if config::is_config_file(&doc.path) => Some("config"),
        _ => None,
    }
}

fn all(index: &Index<Vec<Name>>) -> impl Iterator<Item = (&str, &Name)> {
    index.iter().flat_map(|(path, names)| names.iter().map(move |n| (path, n)))
}

fn locations<'a>(found: impl Iterator<Item = (&'a str, &'a Name)>) -> Vec<Location> {
    found.map(|(path, n)| Location { path: path.into(), range: n.range }).collect()
}

fn completions<'a>(names: impl Iterator<Item = &'a str>, detail: &str, replace: Option<Range>) -> Vec<CompletionItem> {
    let names: BTreeSet<&str> = names.filter(|n| !n.is_empty() && !beans::is_expression(n)).collect();
    names
        .into_iter()
        .map(|n| CompletionItem { detail: Some(detail.into()), replace, ..CompletionItem::new(n, CompletionKind::Reference) })
        .collect()
}

pub struct Batch {
    indexes: Indexes<Vec<Name>>,
}

impl Default for Batch {
    fn default() -> Self {
        Self { indexes: Indexes::new(&beans::GLOBS, index_file) }
    }
}

impl Batch {
    fn index(&mut self, doc: &Document) -> Option<&Index<Vec<Name>>> {
        self.indexes.for_document(&doc.path, &doc.text, PROVIDER_BUDGET).map(|i| &*i)
    }
}

/// The XML link under the caret, with the names of its file.
fn xml_link(doc: &Document, at: Position) -> Option<(Link, Vec<Name>)> {
    let (names, links) = parse_xml(&doc.text);
    let link = links.into_iter().find(|l| contains(l.range, at))?;
    Some((link, names))
}

/// The names of the other files of the project.
fn elsewhere<'a>(index: &'a Index<Vec<Name>>, doc: &Document) -> Vec<(&'a str, &'a Name)> {
    all(index).filter(|(p, _)| *p != doc.path).collect()
}

/// Where an XML link may lead: the same job first, then the file, then the project.
fn targets(link: &Link, names: &[Name], project: &[(&str, &Name)], path: &str) -> Vec<Location> {
    let here = |n: &Name| Location { path: path.into(), range: n.range };
    let elsewhere = |kinds: &[Kind]| {
        locations(project.iter().copied().filter(|(_, n)| kinds.contains(&n.kind) && n.name == link.value))
    };
    if link.kind == LinkKind::Bean {
        let mut found: Vec<_> = names.iter().filter(|n| n.kind == Kind::Bean && n.name == link.value).map(here).collect();
        found.extend(elsewhere(&[Kind::Bean]));
        return found;
    }
    let kinds = [Kind::State, Kind::Job];
    let named = |n: &&Name| kinds.contains(&n.kind) && n.name == link.value;
    let same_job: Vec<_> =
        names.iter().filter(named).filter(|n| link.kind == LinkKind::Target && n.scope == link.scope).map(here).collect();
    if !same_job.is_empty() {
        return same_job;
    }
    let in_file: Vec<_> = names.iter().filter(named).map(here).collect();
    if !in_file.is_empty() {
        return in_file;
    }
    elsewhere(&kinds)
}

fn xml_problems(text: &str, index: &Index<Vec<Name>>) -> Vec<Diagnostic> {
    let (names, links) = parse_xml(text);
    let mut out = Vec::new();
    for l in links.iter().filter(|l| !l.value.is_empty() && !beans::is_expression(&l.value)) {
        match l.kind {
            LinkKind::Target if l.scope.is_some() => {
                if !names.iter().any(|n| n.kind == Kind::State && n.scope == l.scope && n.name == l.value) {
                    out.push(
                        Diagnostic::new(l.range, Severity::Error, format!("This job has no step, flow, split or decision '{}'.", l.value))
                            .code("unknown-transition-target"),
                    );
                }
            }
            LinkKind::Bean if index.complete() => {
                let single = !l.value.contains([',', ' ']);
                let known = all(index).any(|(_, n)| n.kind == Kind::Bean && n.name == l.value);
                if single && !known && !beans::INFRASTRUCTURE.contains(&l.value.as_str()) {
                    out.push(
                        Diagnostic::new(l.range, Severity::Warning, format!("No bean named '{}' is declared in the project.", l.value))
                            .code("unknown-bean"),
                    );
                }
            }
            _ => {}
        }
    }
    out
}

fn config_problems(doc: &Document, index: &Index<Vec<Name>>) -> Vec<Diagnostic> {
    if !index.complete() {
        return Vec::new();
    }
    job_name_items(&doc.path, &doc.text)
        .into_iter()
        .filter(|(n, _, _)| !n.is_empty() && !beans::is_expression(n))
        .filter(|(n, _, _)| !all(index).any(|(_, x)| x.kind == Kind::Job && x.name == *n))
        .map(|(n, from, to)| {
            Diagnostic::new(range(&doc.text, from, to), Severity::Warning, format!("No job named '{n}' is declared in the project."))
                .code("unknown-job")
        })
        .collect()
}

impl Providers for Batch {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        match file_kind(doc) {
            Some("xml") => {
                let Some((link, names)) = xml_link(doc, at) else { return Vec::new() };
                let Some(index) = self.index(doc) else { return Vec::new() };
                let (kinds, detail) = match link.kind {
                    LinkKind::Target => (&[Kind::State][..], "step, flow, split or decision"),
                    LinkKind::Parent => (&[Kind::State, Kind::Job][..], "step, flow or job"),
                    LinkKind::Bean => (&[Kind::Bean][..], "bean"),
                };
                let local = names
                    .iter()
                    .filter(|n| kinds.contains(&n.kind) && (link.kind != LinkKind::Target || n.scope == link.scope));
                let elsewhere = elsewhere(index, doc)
                    .into_iter()
                    .map(|(_, n)| n)
                    .filter(|n| link.kind != LinkKind::Target && kinds.contains(&n.kind));
                completions(local.chain(elsewhere).map(|n| n.name.as_str()), detail, Some(link.range))
            }
            Some("config") => {
                let caret = offset(&doc.text, at).unwrap_or(0);
                let item = job_name_items(&doc.path, &doc.text).into_iter().find(|(_, from, to)| *from <= caret && caret <= *to);
                // An empty YAML value ends at its colon: a caret after it on the line still counts.
                let on_key = config::keys(&doc.path, &doc.text).into_iter().any(|k| {
                    config::canonical(&k.key) == config::canonical(JOB_NAME_KEY)
                        && k.end <= caret
                        && doc.text.get(k.end..caret).is_some_and(|s| !s.contains('\n'))
                });
                if item.is_none() && !on_key {
                    return Vec::new();
                }
                let replace = item.map(|(_, from, to)| range(&doc.text, from, to));
                let Some(index) = self.index(doc) else { return Vec::new() };
                completions(all(index).filter(|(_, n)| n.kind == Kind::Job).map(|(_, n)| n.name.as_str()), "batch job", replace)
            }
            _ => Vec::new(),
        }
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        match file_kind(doc) {
            Some("xml") => {
                let Some((link, names)) = xml_link(doc, at) else { return Vec::new() };
                let Some(index) = self.index(doc) else { return Vec::new() };
                targets(&link, &names, &elsewhere(index, doc), &doc.path)
            }
            Some("config") => {
                let caret = offset(&doc.text, at).unwrap_or(0);
                let Some((name, _, _)) = job_name_items(&doc.path, &doc.text).into_iter().find(|(_, f, t)| *f <= caret && caret <= *t)
                else {
                    return Vec::new();
                };
                let Some(index) = self.index(doc) else { return Vec::new() };
                locations(all(index).filter(|(_, n)| n.kind == Kind::Job && n.name == name))
            }
            _ => Vec::new(),
        }
    }

    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let kind = file_kind(doc);
        let Some(index) = kind.and_then(|_| self.index(doc)) else { return Vec::new() };
        let problems = match kind {
            Some("xml") => xml_problems(&doc.text, index),
            Some("java") => java_problems(&doc.text, all(index).any(|(_, n)| n.kind == Kind::BootApp)),
            _ => config_problems(doc, index),
        };
        let _ = host::publish_diagnostics(&doc.path, &problems);
        Vec::new()
    }
}

impl Plugin for Batch {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

register_plugin!(Batch, providers);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::text::position;

    const JOB_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<beans:beans xmlns="http://www.springframework.org/schema/batch"
             xmlns:beans="http://www.springframework.org/schema/beans">
    <job id="importJob" job-repository="jobRepository">
        <step id="load" next="check">
            <tasklet transaction-manager="transactionManager">
                <chunk reader="csvReader" writer="dbWriter" processor="missingProcessor" commit-interval="10"/>
            </tasklet>
        </step>
        <decision id="check" decider="importDecider">
            <next on="FAILED" to="report"/>
            <next on="*" to="lost"/>
        </decision>
        <step id="report" parent="baseStep">
            <tasklet ref="reportTasklet"/>
            <stop on="RETRY" restart="load"/>
        </step>
    </job>
    <step id="baseStep" abstract="true"/>
    <beans:bean id="csvReader" class="com.acme.CsvReader"/>
</beans:beans>
"#;
    const JOB_CONFIG: &str = r#"package com.acme;

import org.springframework.batch.core.configuration.annotation.EnableBatchProcessing;
import org.springframework.batch.core.configuration.annotation.JobBuilderFactory;

@Configuration
@EnableBatchProcessing
public class BatchConfig {
    @Autowired private JobBuilderFactory jobs;

    @Bean
    public Job exportJob(JobRepository repo, Step step) {
        return new JobBuilder("exportJob", repo).start(step).build();
    }

    @Bean
    public Job legacyJob() {
        return jobBuilderFactory.get("legacyJob").start(step()).build();
    }

    @Bean ItemWriter<Row> dbWriter() { return null; }
    @Bean Tasklet reportTasklet() { return null; }
    @Bean JobExecutionDecider importDecider() { return null; }
}
"#;
    const APP: &str = "package com.acme;\n\n@SpringBootApplication\npublic class App {}\n";
    const XML_PATH: &str = "/project/src/main/resources/jobs/import.xml";
    const JAVA_PATH: &str = "/project/src/main/java/com/acme/BatchConfig.java";

    fn setup() -> MockHost {
        let host = MockHost::install();
        host.add_file("src/main/resources/jobs/import.xml", JOB_XML);
        host.add_file("src/main/java/com/acme/BatchConfig.java", JOB_CONFIG);
        host.add_file("src/main/java/com/acme/App.java", APP);
        host
    }

    fn doc(path: &str, language: &str, text: &str) -> Document {
        Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
    }

    fn at(text: &str, needle: &str) -> Position {
        position(text, text.find(needle).unwrap() + 1)
    }

    #[test]
    fn reads_jobs_states_and_links() {
        let (names, links) = parse_xml(JOB_XML);
        let named = |kind| names.iter().filter(|n| n.kind == kind).map(|n| n.name.as_str()).collect::<Vec<_>>();
        assert_eq!(named(Kind::Job), ["importJob"]);
        assert_eq!(named(Kind::State), ["load", "check", "report", "baseStep"]);
        assert_eq!(named(Kind::Bean), ["importJob", "baseStep", "csvReader"]);
        let targets: Vec<_> = links.iter().filter(|l| l.kind == LinkKind::Target).map(|l| l.value.as_str()).collect();
        assert_eq!(targets, ["check", "report", "lost", "load"]);
        assert!(parse_xml("<beans><job id=\"x\"/></beans>").0.is_empty());

        let java: Vec<_> = parse_source(JAVA_PATH, JOB_CONFIG).into_iter().filter(|n| n.kind == Kind::Job).map(|n| n.name).collect();
        assert_eq!(java, ["exportJob", "legacyJob"], "builder names, not bean names");
    }

    #[test]
    fn navigates_transitions_parents_and_beans() {
        let _host = setup();
        let mut b = Batch::default();
        let d = doc(XML_PATH, "xml", JOB_XML);
        let line = |found: Vec<Location>| found.iter().map(|l| (l.path.rsplit('/').next().unwrap().to_owned(), l.range.start.line)).collect::<Vec<_>>();
        assert_eq!(line(b.definition(&d, at(JOB_XML, "\"check\""))), [("import.xml".into(), 9)]);
        assert_eq!(line(b.definition(&d, at(JOB_XML, "\"load\"/>"))), [("import.xml".into(), 4)]);
        assert_eq!(line(b.definition(&d, at(JOB_XML, "\"baseStep\""))), [("import.xml".into(), 18)]);
        assert_eq!(line(b.definition(&d, at(JOB_XML, "\"dbWriter\""))), [("BatchConfig.java".into(), 20)]);
        assert_eq!(line(b.definition(&d, at(JOB_XML, "\"csvReader\""))), [("import.xml".into(), 19)]);
    }

    #[test]
    fn completes_states_and_beans() {
        let _host = setup();
        let mut b = Batch::default();
        let d = doc(XML_PATH, "xml", JOB_XML);
        let labels = |items: Vec<CompletionItem>| items.into_iter().map(|i| i.label).collect::<Vec<_>>();
        assert_eq!(labels(b.completion(&d, at(JOB_XML, "\"lost\""), None)), ["check", "load", "report"]);
        let beans = labels(b.completion(&d, at(JOB_XML, "\"csvReader\""), None));
        assert!(beans.contains(&"dbWriter".to_owned()) && beans.contains(&"csvReader".to_owned()) && beans.contains(&"app".to_owned()));
    }

    #[test]
    fn reports_unknown_targets_and_beans() {
        let host = setup();
        let mut b = Batch::default();
        b.line_markers(&doc(XML_PATH, "xml", JOB_XML));
        let found: Vec<_> =
            host.diagnostics()[XML_PATH].iter().map(|d| (d.range.start.line, d.code.clone().unwrap(), d.severity)).collect();
        assert_eq!(found, [
            (6, "unknown-bean".into(), Severity::Warning),
            (11, "unknown-transition-target".into(), Severity::Error),
        ]);
        assert!(host.diagnostics()[XML_PATH][0].message.contains("'missingProcessor'"));
    }

    #[test]
    fn flags_spring_batch_5_migrations() {
        let host = setup();
        let mut b = Batch::default();
        b.line_markers(&doc(JAVA_PATH, "java", JOB_CONFIG));
        let found: Vec<_> = host.diagnostics()[JAVA_PATH]
            .iter()
            .map(|d| (d.range.start.line, d.code.clone().unwrap(), d.deprecated))
            .collect();
        assert_eq!(found, [
            (8, "deprecated-builder-factory".into(), true),
            (17, "deprecated-builder-factory".into(), true),
            (6, "enable-batch-processing-with-boot".into(), false),
        ]);
        let get = &host.diagnostics()[JAVA_PATH][1];
        assert_eq!((get.range.start.character, get.range.end.character), (15, 36));
        assert!(get.message.contains("new JobBuilder(name, jobRepository)"));

        let no_boot = java_problems(JOB_CONFIG, false);
        assert!(no_boot.iter().all(|d| d.code.as_deref() != Some("enable-batch-processing-with-boot")));
    }

    #[test]
    fn links_the_job_name_property() {
        let host = setup();
        let mut b = Batch::default();
        b.line_markers(&doc(XML_PATH, "xml", JOB_XML));
        let props = "spring.batch.job.name=importJob, nightlyJob\n";
        let path = "/project/src/main/resources/application.properties";
        let d = doc(path, "properties", props);
        let found = b.definition(&d, at(props, "importJob"));
        assert_eq!((found[0].path.as_str(), found[0].range.start.line), (XML_PATH, 3));
        b.line_markers(&d);
        let problems = &host.diagnostics()[path];
        assert_eq!(problems.len(), 1);
        assert!(problems[0].message.contains("'nightlyJob'"));
        assert_eq!(problems[0].range.start.character, 33);

        let yaml = "spring:\n  batch:\n    job:\n      name: \n";
        let d = doc("/project/src/main/resources/application.yml", "yaml", yaml);
        let items = b.completion(&d, Position::new(3, 12), None);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["exportJob", "importJob", "legacyJob"]);
        assert!(b.completion(&doc(path, "properties", "server.port=1\n"), Position::new(0, 13), None).is_empty());
    }
}
