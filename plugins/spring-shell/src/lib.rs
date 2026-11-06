//! Spring Shell Tools for OpenIDEA: indexes the shell commands of a project
//! (`@ShellMethod` and `@Command` methods), checks availability methods and
//! providers, and completes and links the names they refer to. Text-based,
//! Java only.

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::{offset, position};
use openidea_plugin::{Diagnostic, Error, Invocation, Location, Plugin, Position, Range, Severity, host, register_plugin};
use spring_text::index::{Index, Indexes, PROVIDER_BUDGET};
use spring_text::java::{self, Annotation, Arg, JavaFile, Member, Str};
use spring_text::{SOURCE_GLOBS, parse_source, range};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// The full command, words separated by one space.
    pub key: String,
    /// Qualified name of the declaring class.
    pub class: String,
    pub method: String,
    /// The key's literal, or the method name when the key is derived from it.
    pub anchor: Range,
    /// The method name.
    pub target: Range,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shell {
    pub commands: Vec<Command>,
    pub groups: Vec<String>,
    /// Bean names, and whether the bean is an `AvailabilityProvider`.
    pub beans: Vec<(String, Range, bool)>,
}

/// Spring Shell's default command name: `addUser` → `add-user`.
pub fn kebab(name: &str) -> String {
    let mut out = String::new();
    let mut lower = false;
    for c in name.chars() {
        if lower && c.is_ascii_uppercase() {
            out.push('-');
        }
        lower = c.is_ascii_lowercase();
        out.push(c.to_ascii_lowercase());
    }
    out
}

fn words(strings: &[&Str]) -> String {
    strings.iter().flat_map(|s| s.value.split_whitespace()).collect::<Vec<_>>().join(" ")
}

/// `@Command` is a common name (picocli has one): only Spring Shell's counts.
fn spring_shell(f: &JavaFile) -> bool {
    f.imports.iter().any(|i| i.path.starts_with("org.springframework.shell."))
}

pub fn commands(f: &JavaFile, text: &str) -> Vec<Command> {
    let new_model = spring_shell(f);
    let mut out = Vec::new();
    for (ti, t) in f.types.iter().enumerate() {
        let class = f.qualified(ti);
        let prefix = t.annotation("Command").filter(|_| new_model).map(|a| words(&a.strings(&["command"])));
        for m in t.methods() {
            let target = range(text, m.at, m.at + m.name.len());
            let mut push = |key: String, anchor: Range| {
                if !key.is_empty() && !key.contains("${") {
                    out.push(Command { key, class: class.clone(), method: m.name.clone(), anchor, target });
                }
            };
            if let Some(a) = m.annotation("ShellMethod") {
                let keys = a.strings(&["key"]);
                if keys.is_empty() {
                    push(kebab(&m.name), target);
                }
                for s in keys {
                    push(words(&[s]), range(text, s.at, s.end));
                }
            } else if new_model && let Some(a) = m.annotation("Command") {
                let own = a.strings(&["command"]);
                let anchor = own.first().map_or(target, |s| range(text, s.at, s.end));
                let own = if own.is_empty() { kebab(&m.name) } else { words(&own) };
                push(format!("{} {own}", prefix.as_deref().unwrap_or_default()).trim().to_owned(), anchor);
            }
        }
    }
    out
}

/// Every annotation of the file's types and members.
fn annotations(f: &JavaFile) -> impl Iterator<Item = &Annotation> {
    f.types.iter().flat_map(|t| t.annotations.iter().chain(t.members.iter().flat_map(|m| &m.annotations)))
}

fn groups(f: &JavaFile) -> Vec<String> {
    let mut out: Vec<String> = annotations(f)
        .flat_map(|a| match a.name.as_str() {
            "ShellCommandGroup" => a.strings(&["value"]),
            "ShellMethod" | "Command" => a.strings(&["group"]),
            _ => Vec::new(),
        })
        .map(|s| s.value.clone())
        .filter(|g| !g.is_empty() && !g.contains("${"))
        .collect();
    out.sort();
    out.dedup();
    out
}

fn beans(f: &JavaFile, text: &str) -> Vec<(String, Range, bool)> {
    let providers: Vec<&str> = f
        .types
        .iter()
        .filter(|t| t.implements.iter().any(|i| i.name == "AvailabilityProvider"))
        .map(|t| t.name.as_str())
        .collect();
    java::bean_names(f)
        .into_iter()
        .map(|b| {
            let provider = b.ty == "AvailabilityProvider" || providers.contains(&b.ty.as_str());
            (b.name, range(text, b.at, b.at + b.len), provider)
        })
        .collect()
}

fn scan(path: &str, text: &str) -> Option<Shell> {
    let f = parse_source(path, text);
    let shell = Shell { commands: commands(&f, text), groups: groups(&f), beans: beans(&f, text) };
    (shell != Shell::default()).then_some(shell)
}

fn all_commands(index: &Index<Shell>) -> impl Iterator<Item = (&str, &Command)> {
    index.iter().flat_map(|(p, s)| s.commands.iter().map(move |c| (p, c)))
}

fn all_beans(index: &Index<Shell>) -> impl Iterator<Item = (&str, &(String, Range, bool))> {
    index.iter().flat_map(|(p, s)| s.beans.iter().map(move |b| (p, b)))
}

fn simple(class: &str) -> &str {
    class.rsplit('.').next().unwrap_or(class)
}

fn problem(range: Range, severity: Severity, code: &str, message: String) -> Diagnostic {
    Diagnostic::new(range, severity, message).code(code)
}

/// The commands an availability value names: those of `class` first, else the project's.
fn named<'a>(index: &'a Index<Shell>, class: &str, key: &str) -> Vec<(&'a str, &'a Command)> {
    let found: Vec<_> = all_commands(index).filter(|(_, c)| c.key == key).collect();
    let own: Vec<_> = found.iter().copied().filter(|(_, c)| c.class == class).collect();
    if own.is_empty() { found } else { own }
}

/// Problems and gutter links of a Java file.
fn analyze(doc: &Document, f: &JavaFile, index: &Index<Shell>) -> (Vec<Diagnostic>, Vec<LineMarker>) {
    let text = &doc.text;
    let at = |from: usize, to: usize| range(text, from, to);
    let name_of = |m: &Member| at(m.at, m.at + m.name.len());
    let line_of = |m: &Member| position(text, m.at).line;
    let mine = commands(f, text);
    let mut problems = Vec::new();
    let mut markers = Vec::new();

    for c in &mine {
        let other = all_commands(index).find(|(p, o)| o.key == c.key && !(*p == doc.path && o.anchor == c.anchor));
        if let Some((_, o)) = other {
            let message = format!("Command `{}` is also declared by {}.{}.", c.key, simple(&o.class), o.method);
            problems.push(problem(c.anchor, Severity::Error, "duplicate-command", message));
        }
    }

    for (ti, t) in f.types.iter().enumerate() {
        let class = f.qualified(ti);
        let legacy: Vec<&Member> = t.methods().filter(|m| m.has(&["ShellMethod"])).collect();
        for m in t.methods() {
            let by_convention = m.name.strip_suffix("Availability").is_some_and(|n| legacy.iter().any(|c| c.name == n));
            let annotation = m.annotation("ShellMethodAvailability");
            if (by_convention || annotation.is_some()) && m.ty.as_ref().is_none_or(|t| t.name != "Availability") {
                let message = format!("Availability method `{}` must return `Availability`.", m.name);
                problems.push(problem(name_of(m), Severity::Error, "availability-return-type", message));
            }
            for s in annotation.map(|a| a.strings(&["value"])).unwrap_or_default() {
                if s.value == "*" || s.value.contains("${") || !named(index, &class, &words(&[s])).is_empty() {
                    continue;
                }
                if index.complete() {
                    let message = format!("No shell command is named `{}`.", s.value);
                    problems.push(problem(at(s.at, s.end), Severity::Error, "unknown-command", message));
                }
            }
            if let Some(a) = m.annotation("CommandAvailability")
                && let Some(s) = a.strings(&["provider"]).first()
                && !s.value.contains("${")
            {
                match all_beans(index).find(|(_, b)| b.0 == s.value) {
                    Some((path, b)) => markers.push(LineMarker {
                        line: line_of(m),
                        icon: None,
                        tooltip: format!("Availability provider: {}", s.value),
                        target: Some(Location { path: path.into(), range: b.1 }),
                    }),
                    None if index.complete() => {
                        let message = format!("No bean is named `{}`.", s.value);
                        problems.push(problem(at(s.at, s.end), Severity::Warning, "unknown-availability-provider", message));
                    }
                    None => {}
                }
            }
        }
        for m in &legacy {
            let keys: Vec<&str> = mine.iter().filter(|c| c.class == class && c.method == m.name).map(|c| c.key.as_str()).collect();
            let conventional = format!("{}Availability", m.name);
            let available = t.methods().find(|a| a.name == conventional).or_else(|| {
                t.methods().find(|a| {
                    a.annotation("ShellMethodAvailability")
                        .is_some_and(|an| an.strings(&["value"]).iter().any(|s| s.value == "*" || keys.contains(&words(&[s]).as_str())))
                })
            });
            if let Some(a) = available {
                markers.push(LineMarker {
                    line: line_of(m),
                    icon: None,
                    tooltip: format!("Availability method: {}()", a.name),
                    target: Some(Location { path: doc.path.clone(), range: name_of(a) }),
                });
            }
        }
    }
    (problems, markers)
}

/// The annotation string literal at byte `at`, with its annotation and argument.
fn literal_at(f: &JavaFile, at: usize) -> Option<(&Annotation, &Arg, &Str)> {
    annotations(f).find_map(|a| a.args.iter().find_map(|arg| Some((a, arg, arg.strings.iter().find(|s| s.holds(at))?))))
}

/// The qualified name of the type declaring byte `at`.
fn class_at(f: &JavaFile, at: usize) -> String {
    f.types.iter().enumerate().rfind(|(_, t)| t.start <= at && at <= t.end).map(|(i, _)| f.qualified(i)).unwrap_or_default()
}

pub struct ShellTools {
    indexes: Indexes<Shell>,
}

impl Default for ShellTools {
    fn default() -> Self {
        Self { indexes: Indexes::new(SOURCE_GLOBS, scan) }
    }
}

impl Providers for ShellTools {
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let f = parse_source(&doc.path, &doc.text);
        let Some(index) = self.indexes.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else { return Vec::new() };
        let (problems, markers) = analyze(doc, &f, index);
        let _ = host::publish_diagnostics(&doc.path, &problems);
        markers
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let f = parse_source(&doc.path, &doc.text);
        let Some(offset) = offset(&doc.text, at) else { return Vec::new() };
        let Some((a, arg, s)) = literal_at(&f, offset) else { return Vec::new() };
        let Some(index) = self.indexes.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else { return Vec::new() };
        match (a.name.as_str(), arg.name.as_str()) {
            ("ShellMethodAvailability", "value") => named(index, &class_at(&f, offset), &words(&[s]))
                .into_iter()
                .map(|(path, c)| Location { path: path.into(), range: c.target })
                .collect(),
            ("CommandAvailability", "provider") => all_beans(index)
                .filter(|(_, b)| b.0 == s.value)
                .map(|(path, b)| Location { path: path.into(), range: b.1 })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let f = parse_source(&doc.path, &doc.text);
        let Some(offset) = offset(&doc.text, at) else { return Vec::new() };
        let Some((a, arg, s)) = literal_at(&f, offset) else { return Vec::new() };
        let replace = Some(range(&doc.text, s.at, s.end));
        let item = |label: &str, kind: CompletionKind, detail: &str, sort: &str| CompletionItem {
            detail: Some(detail.into()),
            replace,
            sort_text: Some(format!("{sort}{label}")),
            ..CompletionItem::new(label, kind)
        };
        let Some(index) = self.indexes.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else { return Vec::new() };
        let mut out: Vec<CompletionItem> = match (a.name.as_str(), arg.name.as_str()) {
            ("ShellMethodAvailability", "value") => {
                let class = class_at(&f, offset);
                let mut out: Vec<_> = all_commands(index)
                    .map(|(_, c)| {
                        let detail = format!("{}.{}()", simple(&c.class), c.method);
                        item(&c.key, CompletionKind::Method, &detail, if c.class == class { "0" } else { "1" })
                    })
                    .collect();
                out.push(item("*", CompletionKind::Keyword, "every command of the class", "2"));
                out
            }
            ("ShellMethod" | "Command", "group") | ("ShellCommandGroup", "value") => index
                .iter()
                .flat_map(|(_, s)| &s.groups)
                .map(|g| item(g, CompletionKind::Module, "command group", "0"))
                .collect(),
            ("CommandAvailability", "provider") => all_beans(index)
                .filter(|(_, b)| b.2)
                .map(|(_, b)| item(&b.0, CompletionKind::Reference, "AvailabilityProvider bean", "0"))
                .collect(),
            _ => Vec::new(),
        };
        out.sort_by(|a, b| a.label.cmp(&b.label));
        out.dedup_by(|a, b| a.label == b.label);
        out
    }
}

impl Plugin for ShellTools {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

register_plugin!(ShellTools, providers);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    const LEGACY: &str = r#"package com.acme.cli;

import org.springframework.shell.standard.ShellComponent;
import org.springframework.shell.standard.ShellMethod;

@ShellComponent
public class UserCommands {
    @ShellMethod(value = "Add a user", group = "Users")
    public String addUser(@ShellOption String name) { return name; }

    @ShellMethod(key = {"remove", "rm"}, value = "Remove a user")
    public void remove(String name) {}

    @ShellMethod("List users")
    public void list() {}

    public Availability addUserAvailability() { return Availability.available(); }

    @ShellMethodAvailability({"remove", "nope"})
    public boolean canRemove() { return true; }

    @ShellMethodAvailability("*")
    public Availability always() { return Availability.available(); }
}
"#;

    const MODERN: &str = r#"package com.acme.cli;

import org.springframework.shell.command.annotation.Command;
import org.springframework.shell.command.annotation.CommandAvailability;

@Command(command = "db", group = "Database")
class DbCommands {
    @Command(command = {"dump", "all"})
    @CommandAvailability(provider = "connected")
    void dumpAll() {}

    @Command
    @CommandAvailability(provider = "missing")
    void showTables() {}

    @Command(command = "list")
    void list() {}
}

@Configuration
class ShellConfig {
    @Bean
    AvailabilityProvider connected() { return () -> Availability.available(); }

    @Bean
    Clock clock() { return null; }
}
"#;

    fn doc(name: &str, text: &str) -> Document {
        Document { path: format!("/project/src/main/java/com/acme/cli/{name}"), language: "java".into(), version: 1, text: text.into() }
    }

    fn setup() -> MockHost {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/cli/UserCommands.java", LEGACY);
        host.add_file("src/main/java/com/acme/cli/DbCommands.java", MODERN);
        host.add_file("src/main/java/com/acme/cli/Other.java", "@ShellComponent class Other { @ShellMethod(key = \"list\") void ls() {} }");
        host
    }

    fn pos(text: &str, needle: &str, delta: usize) -> Position {
        position(text, text.find(needle).unwrap() + delta)
    }

    const KOTLIN: &str = r#"package com.acme.cli

import org.springframework.shell.command.annotation.Command
import org.springframework.shell.command.annotation.CommandAvailability

@ShellComponent
class KtCommands {
    @ShellMethod(key = ["greet", "hi"], value = "Say hello")
    suspend fun greet(name: String): String = "hi $name"

    @Command(command = ["kt", "dump"], group = "Kt")
    @CommandAvailability(provider = "ktMissing")
    fun dump() {}

    @ShellMethodAvailability(["greet", "nope"])
    fun canGreet(): Availability = Availability.available()
}

@Configuration
class KtConfig {
    @Bean
    fun ktConnected(): AvailabilityProvider = AvailabilityProvider { Availability.available() }
}
"#;

    #[test]
    fn kotlin_shell_command_is_indexed() {
        let kt = |text: &str| commands(&parse_source("K.kt", text), text).into_iter().map(|c| c.key).collect::<Vec<_>>();
        assert_eq!(kt(KOTLIN), ["greet", "hi", "kt dump"]);
        let beans = scan("K.kt", KOTLIN).unwrap().beans;
        assert!(beans.iter().any(|b| b.0 == "ktConnected" && b.2), "{beans:?}");
        let host = setup();
        host.add_file("src/main/kotlin/com/acme/cli/KtCommands.kt", KOTLIN);
        let mut tools = ShellTools::default();
        let d = Document {
            path: "/project/src/main/kotlin/com/acme/cli/KtCommands.kt".into(),
            language: "kotlin".into(),
            version: 1,
            text: KOTLIN.into(),
        };
        tools.line_markers(&d);
        let problems = &host.diagnostics()["/project/src/main/kotlin/com/acme/cli/KtCommands.kt"];
        let messages: Vec<_> = problems.iter().map(|p| p.message.as_str()).collect();
        assert!(messages.iter().any(|m| m.contains("nope")), "{messages:?}");
        assert!(messages.iter().any(|m| m.contains("ktMissing")), "{messages:?}");
        assert_eq!(messages.len(), 2, "{messages:?}");
    }

    #[test]
    fn names_commands_of_both_models() {
        assert_eq!(kebab("addUser"), "add-user");
        assert_eq!(kebab("getURL"), "get-url");
        let keys = |text: &str| commands(&java::parse(text), text).into_iter().map(|c| c.key).collect::<Vec<_>>();
        assert_eq!(keys(LEGACY), ["add-user", "remove", "rm", "list"]);
        assert_eq!(keys(MODERN), ["db dump all", "db show-tables", "db list"]);
        assert!(keys("import picocli.CommandLine.Command;\n@Command(name = \"x\") class A { @Command void run() {} }").is_empty());
        let shell = scan("", MODERN).unwrap();
        assert_eq!(shell.groups, ["Database"]);
        assert_eq!(shell.beans.iter().map(|b| (b.0.as_str(), b.2)).collect::<Vec<_>>(), [
            ("shellConfig", false),
            ("connected", true),
            ("clock", false)
        ]);
    }

    #[test]
    fn checks_availability_and_duplicates() {
        let host = setup();
        let mut tools = ShellTools::default();
        let markers = tools.line_markers(&doc("UserCommands.java", LEGACY));
        let tips: Vec<_> = markers.iter().map(|m| (m.line, m.tooltip.as_str())).collect();
        assert_eq!(tips, [(8, "Availability method: addUserAvailability()"), (11, "Availability method: canRemove()"), (14, "Availability method: always()")]);
        assert_eq!(markers[1].target.as_ref().unwrap().range.start, pos(LEGACY, "canRemove", 0));

        let problems = &host.diagnostics()["/project/src/main/java/com/acme/cli/UserCommands.java"];
        let codes: Vec<_> = problems.iter().map(|p| (p.code.as_deref().unwrap(), p.range.start.line)).collect();
        assert_eq!(codes, [("duplicate-command", 14), ("availability-return-type", 19), ("unknown-command", 18)]);
        assert_eq!(problems[2].range.start, pos(LEGACY, "nope", 0));
    }

    #[test]
    fn checks_availability_providers() {
        let host = setup();
        let mut tools = ShellTools::default();
        let markers = tools.line_markers(&doc("DbCommands.java", MODERN));
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].line, 9);
        assert_eq!(markers[0].target.as_ref().unwrap().range.start, Position::new(22, 25));
        let problems = &host.diagnostics()["/project/src/main/java/com/acme/cli/DbCommands.java"];
        let codes: Vec<_> = problems.iter().map(|p| (p.code.as_deref().unwrap(), p.severity)).collect();
        assert_eq!(codes, [("unknown-availability-provider", Severity::Warning)]);
    }

    #[test]
    fn completes_names() {
        let _host = setup();
        let mut tools = ShellTools::default();
        let labels = |items: Vec<CompletionItem>| items.into_iter().map(|i| i.label).collect::<Vec<_>>();
        let d = doc("UserCommands.java", LEGACY);
        let at = pos(LEGACY, "\"nope\"", 1);
        assert_eq!(labels(tools.completion(&d, at, None)), ["*", "add-user", "db dump all", "db list", "db show-tables", "list", "remove", "rm"]);
        assert_eq!(labels(tools.completion(&d, pos(LEGACY, "\"Users\"", 1), None)), ["Database", "Users"]);
        let d = doc("DbCommands.java", MODERN);
        assert_eq!(labels(tools.completion(&d, pos(MODERN, "\"missing\"", 1), None)), ["connected"]);
        assert!(tools.completion(&d, pos(MODERN, "\"list\"", 1), None).is_empty());
    }

    #[test]
    fn navigates_from_names() {
        let _host = setup();
        let mut tools = ShellTools::default();
        let found = tools.definition(&doc("UserCommands.java", LEGACY), pos(LEGACY, "\"remove\", \"nope\"", 2));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].range.start, Position::new(11, 16));
        let found = tools.definition(&doc("DbCommands.java", MODERN), pos(MODERN, "\"connected\"", 3));
        assert_eq!(found[0].range.start, Position::new(22, 25));
    }
}
