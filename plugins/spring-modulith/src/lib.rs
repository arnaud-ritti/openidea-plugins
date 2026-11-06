//! Spring Modulith Tools for OpenIDEA: finds the application modules of a
//! project (direct sub-packages of the `@SpringBootApplication`/`@Modulithic`
//! package), warns about imports of another module's internal types and of
//! modules a module doesn't allow, and lists the modules in a tool window.
//! Text-based, Java only: imports are what counts, not fully qualified uses.

use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Diagnostic, Error, Invocation, Location, NotificationLevel, Plugin, Position, Range, Severity, TextEdit, host, register_plugin,
};
use spring_text::diagram::{self, Edge, Node};
use spring_text::index::{COMMAND_BUDGET, Index, Indexes, PROVIDER_BUDGET};
use spring_text::java::{self, Annotation, JavaFile, Str};
use spring_text::tree::Expansion;
use spring_text::{contains, range};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

const DIAGRAM_VIEW: &str = "diagram";
const TITLE: &str = "Spring Modulith";
const OPEN: &str = "type = ApplicationModule.Type.OPEN";

/// What the model needs of a `package-info.java`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Info {
    /// `@NamedInterface` on the package: its name.
    pub named_interface: Option<String>,
    /// `@ApplicationModule` on the package.
    pub module: Option<ModuleDecl>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModuleDecl {
    pub display_name: Option<String>,
    pub allowed: Option<Vec<String>>,
    pub open: bool,
    pub range: Range,
    /// Adds `type = OPEN` when the annotation has no `type`.
    pub open_fix: Option<TextEdit>,
}

/// What the model needs of one Java file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Unit {
    pub package: String,
    pub imports: Vec<String>,
    /// Top-level types: name, name range, `@NamedInterface` name.
    pub types: Vec<(String, Range, Option<String>)>,
    /// Application root packages declared here (`@SpringBootApplication`, `@Modulithic` + `additionalPackages`).
    pub roots: Vec<String>,
    pub info: Option<Info>,
}

fn last_segment(package: &str) -> &str {
    package.rsplit('.').next().unwrap_or(package)
}

/// A `@NamedInterface`'s name: its value, else the package's last segment.
fn interface_name(a: &Annotation, package: &str) -> String {
    a.strings(&["value", "name"]).first().map_or_else(|| last_segment(package).to_owned(), |s| s.value.clone())
}

fn module_decl(a: &Annotation, text: &str) -> ModuleDecl {
    let open_fix = a.arg("type").is_none().then(|| {
        let (at, insert) = if text[..a.end].ends_with(')') {
            (a.end - 1, if a.args.is_empty() { OPEN.to_owned() } else { format!(", {OPEN}") })
        } else {
            (a.end, format!("({OPEN})"))
        };
        TextEdit { range: range(text, at, at), text: insert }
    });
    ModuleDecl {
        display_name: a.strings(&["displayName"]).first().map(|s| s.value.clone()),
        allowed: a.arg("allowedDependencies").map(|arg| arg.strings.iter().map(|s| s.value.clone()).collect()),
        open: a.arg("type").is_some_and(|t| t.text.ends_with("OPEN")),
        range: range(text, a.at, a.end),
        open_fix,
    }
}

pub fn scan(path: &str, text: &str) -> Option<Unit> {
    let f = java::parse(text);
    if f.package.is_empty() {
        return None;
    }
    let types = f
        .types
        .iter()
        .filter(|t| t.outer.is_none())
        .map(|t| {
            let named = t.annotation("NamedInterface").map(|a| interface_name(a, &f.package));
            (t.name.clone(), range(text, t.at, t.at + t.name.len()), named)
        })
        .collect();
    let mut roots = Vec::new();
    for t in f.types.iter().filter(|t| t.has(&["SpringBootApplication", "Modulithic"])) {
        roots.push(f.package.clone());
        let more = t.annotation("Modulithic").map(|a| a.strings(&["additionalPackages"])).unwrap_or_default();
        roots.extend(more.into_iter().map(|s| s.value.clone()));
    }
    let info = path.ends_with("package-info.java").then(|| Info {
        named_interface: f.package_annotations.iter().find(|a| a.name == "NamedInterface").map(|a| interface_name(a, &f.package)),
        module: f.package_annotations.iter().find(|a| a.name == "ApplicationModule").map(|a| module_decl(a, text)),
    });
    let imports = f.imports.iter().map(|i| i.path.clone()).collect();
    Some(Unit { package: f.package, imports, types, roots, info })
}

/// Tests may reach into modules; Spring Modulith verifies production code.
fn is_test(path: &str) -> bool {
    path.replace('\\', "/").contains("/src/test/")
}

#[derive(Clone, Debug, Default)]
pub struct Module {
    pub name: String,
    pub base: String,
    pub display_name: Option<String>,
    pub open: bool,
    pub allowed: Option<Vec<String>>,
    /// Named interfaces: name and package (its sub-packages included).
    pub interfaces: Vec<(String, String)>,
    /// Types exposed one by one with `@NamedInterface`: interface name and qualified type name.
    pub exposed: Vec<(String, String)>,
    /// The package-info's `@ApplicationModule`, else the first type.
    pub location: Option<Location>,
    /// The package-info edit making the module open.
    pub open_fix: Option<(String, TextEdit)>,
}

impl Module {
    fn interface_names(&self) -> BTreeSet<&str> {
        self.interfaces.iter().chain(&self.exposed).map(|(n, _)| n.as_str()).collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Violation {
    /// A type outside the module's API and named interfaces.
    Internal,
    /// A dependency the using module's `allowedDependencies` doesn't list: `b` or `b::iface`.
    NotAllowed(String),
}

#[derive(Default)]
pub struct Model {
    pub roots: Vec<String>,
    pub modules: BTreeMap<String, Module>,
    packages: BTreeSet<String>,
}

impl Model {
    pub fn build(index: &Index<Unit>) -> Self {
        let units: Vec<(&str, &Unit)> = index.iter().filter(|(p, _)| !is_test(p)).collect();
        let mut model = Model {
            roots: units.iter().find(|(_, u)| !u.roots.is_empty()).map(|(_, u)| u.roots.clone()).unwrap_or_default(),
            ..Model::default()
        };
        for (path, u) in units {
            model.packages.insert(u.package.clone());
            let Some((name, base)) = model.module_of(&u.package) else { continue };
            let m = model.modules.entry(name.clone()).or_insert_with(|| Module { name, base, ..Module::default() });
            for (ty, r, named) in &u.types {
                m.location.get_or_insert_with(|| Location { path: path.into(), range: *r });
                if let Some(n) = named {
                    m.exposed.push((n.clone(), format!("{}.{ty}", u.package)));
                }
            }
            let Some(info) = &u.info else { continue };
            if let Some(n) = &info.named_interface {
                m.interfaces.push((n.clone(), u.package.clone()));
            }
            if let Some(d) = info.module.as_ref().filter(|_| u.package == m.base) {
                m.display_name = d.display_name.clone();
                m.allowed = d.allowed.clone();
                m.open = d.open;
                m.location = Some(Location { path: path.into(), range: d.range });
                m.open_fix = d.open_fix.clone().map(|e| (path.to_owned(), e));
            }
        }
        model
    }

    /// The module holding `package`: its name and base package.
    pub fn module_of(&self, package: &str) -> Option<(String, String)> {
        self.roots.iter().find_map(|root| {
            let name = package.strip_prefix(root.as_str())?.strip_prefix('.')?.split('.').next()?;
            Some((name.to_owned(), format!("{root}.{name}")))
        })
    }

    /// The module an import of module `from` reaches into, and what's wrong with it.
    pub fn check(&self, from: &str, import: &str) -> Option<(&Module, Option<Violation>)> {
        let segments: Vec<&str> = import.split('.').filter(|s| *s != "*").collect();
        let split = segments.iter().position(|s| !s.starts_with(|c: char| c.is_lowercase())).unwrap_or(segments.len());
        let package = segments[..split].join(".");
        if !self.packages.contains(&package) {
            return None;
        }
        let (name, _) = self.module_of(&package)?;
        let b = self.modules.get(&name).filter(|b| b.name != from)?;
        let qualified = segments.get(split).map(|ty| format!("{package}.{ty}"));
        let interface = b
            .interfaces
            .iter()
            .find(|(_, p)| package == *p || package.starts_with(&format!("{p}.")))
            .or_else(|| b.exposed.iter().find(|(_, q)| Some(q) == qualified.as_ref()))
            .map(|(n, _)| n.as_str())
            .or_else(|| (package == b.base || b.open).then_some(""));
        let Some(interface) = interface else { return Some((b, Some(Violation::Internal))) };
        let wanted = if interface.is_empty() { b.name.clone() } else { format!("{}::{interface}", b.name) };
        let allowed = self.modules.get(from).and_then(|a| a.allowed.as_ref()).is_none_or(|list| {
            list.iter().map(|d| d.split_whitespace().collect::<String>()).any(|d| d == wanted || d == format!("{}::*", b.name))
        });
        Some((b, (!allowed).then_some(Violation::NotAllowed(wanted))))
    }

    /// Whether an `allowedDependencies` entry names a module (and interface) of the project.
    fn knows(&self, dependency: &str) -> bool {
        let d: String = dependency.split_whitespace().collect();
        let (module, interface) = d.split_once("::").map_or((d.as_str(), None), |(m, i)| (m, Some(i)));
        self.modules.get(module).is_some_and(|m| interface.is_none_or(|i| i == "*" || m.interface_names().contains(i)))
    }
}

/// The problems of a Java file: `(range, module reached, violation)` per import.
fn violations<'m>(model: &'m Model, path: &str, f: &JavaFile, text: &str) -> Vec<(Range, &'m Module, Violation)> {
    let Some((from, _)) = model.module_of(&f.package).filter(|_| !is_test(path)) else { return Vec::new() };
    f.imports
        .iter()
        .filter_map(|i| {
            let (b, v) = model.check(&from, &i.path)?;
            Some((range(text, i.at, i.end), b, v?))
        })
        .collect()
}

fn message(import: &str, b: &Module, v: &Violation, from: &str) -> String {
    match v {
        Violation::Internal => {
            let what = import.strip_suffix(".*").unwrap_or(last_segment(import));
            format!("`{what}` is internal to module `{}`: use its API package or a named interface.", b.name)
        }
        Violation::NotAllowed(d) => format!("Module `{from}` doesn't list `{d}` in its allowedDependencies."),
    }
}

/// The `allowedDependencies` literals of a `package-info.java`.
fn allowed_literals(f: &JavaFile) -> Vec<&Str> {
    f.package_annotations.iter().filter(|a| a.name == "ApplicationModule").flat_map(|a| a.strings(&["allowedDependencies"])).collect()
}

/// Nothing until the project has been read once: a named interface not read yet would look internal.
fn diagnostics(model: &Model, complete: bool, path: &str, f: &JavaFile, text: &str) -> Vec<Diagnostic> {
    if !complete {
        return Vec::new();
    }
    let from = model.module_of(&f.package).map(|m| m.0).unwrap_or_default();
    let mut out: Vec<Diagnostic> = violations(model, path, f, text)
        .into_iter()
        .map(|(r, b, v)| {
            let code = if v == Violation::Internal { "module-internal" } else { "module-not-allowed" };
            Diagnostic::new(r, Severity::Warning, message(&text_of(text, r), b, &v, &from)).code(code)
        })
        .collect();
    if path.ends_with("package-info.java") {
        for s in allowed_literals(f).into_iter().filter(|s| !model.knows(&s.value)) {
            let message = format!("`{}` is not an application module or named interface.", s.value);
            out.push(Diagnostic::new(range(text, s.at, s.end), Severity::Warning, message).code("unknown-module"));
        }
    }
    out
}

fn text_of(text: &str, r: Range) -> String {
    let from = offset(text, r.start).unwrap_or(0);
    let to = offset(text, r.end).unwrap_or(from);
    text[from..to].to_owned()
}

pub struct ModulithTools {
    indexes: Indexes<Unit>,
    expansion: Expansion,
}

impl Default for ModulithTools {
    fn default() -> Self {
        Self { indexes: Indexes::new(&["**/*.java"], scan), expansion: Expansion::default() }
    }
}

impl Providers for ModulithTools {
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let Some(index) = self.indexes.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else { return Vec::new() };
        let model = Model::build(index);
        let problems = diagnostics(&model, index.complete(), &doc.path, &java::parse(&doc.text), &doc.text);
        let _ = host::publish_diagnostics(&doc.path, &problems);
        Vec::new()
    }

    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if !doc.path.ends_with("package-info.java") {
            return Vec::new();
        }
        let f = java::parse(&doc.text);
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let Some(s) = allowed_literals(&f).into_iter().find(|s| s.holds(at)) else { return Vec::new() };
        let Some(index) = self.indexes.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else { return Vec::new() };
        let model = Model::build(index);
        let own = model.module_of(&f.package).map(|m| m.0);
        let replace = Some(range(&doc.text, s.at, s.end));
        let mut out = Vec::new();
        for m in model.modules.values().filter(|m| Some(&m.name) != own.as_ref()) {
            let detail = m.display_name.clone().unwrap_or_else(|| m.base.clone());
            out.push(CompletionItem { detail: Some(detail), replace, ..CompletionItem::new(&m.name, CompletionKind::Module) });
            for i in m.interface_names() {
                let label = format!("{}::{i}", m.name);
                out.push(CompletionItem {
                    detail: Some("named interface".into()),
                    replace,
                    ..CompletionItem::new(&label, CompletionKind::Interface)
                });
            }
        }
        out
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        let Some(index) = self.indexes.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else { return Vec::new() };
        let model = Model::build(index);
        violations(&model, &doc.path, &java::parse(&doc.text), &doc.text)
            .into_iter()
            .filter(|(r, _, v)| *v == Violation::Internal && contains(*r, selection.start))
            .filter_map(|(_, b, _)| {
                let (path, edit) = b.open_fix.clone()?;
                Some(CodeAction {
                    title: format!("Make module `{}` open (exposes all its types)", b.name),
                    kind: Some("quickfix".into()),
                    preferred: false,
                    edits: vec![(path, vec![edit])],
                    data: None,
                })
            })
            .collect()
    }
}

/// Activation key of a tree item: the module to open, then what makes the key unique.
fn key(open: &str, rest: &str) -> String {
    format!("open:{open}#{rest}")
}

type Dependencies<'m> = BTreeMap<&'m str, BTreeMap<&'m str, Option<Violation>>>;

/// For each module, the modules it imports from and the worst violation of those imports.
fn dependencies<'m>(model: &'m Model, index: &Index<Unit>) -> Dependencies<'m> {
    let mut dependencies: Dependencies = BTreeMap::new();
    for (_, u) in index.iter().filter(|(p, _)| !is_test(p)) {
        let Some((from, _)) = model.module_of(&u.package) else { continue };
        let Some(a) = model.modules.get(&from) else { continue };
        let found = dependencies.entry(a.name.as_str()).or_default();
        for i in &u.imports {
            if let Some((b, v)) = model.check(&from, i) {
                let worst = found.entry(b.name.as_str()).or_insert(None);
                if worst.is_none() {
                    *worst = v;
                }
            }
        }
    }
    dependencies
}

fn violation_label(v: &Violation) -> String {
    match v {
        Violation::Internal => "internal types".into(),
        Violation::NotAllowed(d) => format!("{d} not allowed"),
    }
}

/// Modules as nodes, one edge per module importing from another; violations are warning edges.
fn diagram_of(model: &Model, index: &Index<Unit>) -> (Vec<Node>, Vec<Edge>) {
    let nodes = model
        .modules
        .values()
        .map(|m| Node::new(&key(&m.name, ""), &m.name).detail(if m.open { "open" } else { "closed" }))
        .collect();
    let edges = dependencies(model, index)
        .into_iter()
        .flat_map(|(from, found)| {
            found.into_iter().map(move |(to, v)| {
                let edge = Edge::new(&key(from, ""), &key(to, "")).warning(v.is_some());
                match v {
                    Some(v) => edge.label(&violation_label(&v)),
                    None => edge,
                }
            })
        })
        .collect();
    (nodes, edges)
}

/// PlantUML component diagram of the modules and their dependencies.
pub fn plantuml(model: &Model, index: &Index<Unit>) -> String {
    let mut out = String::from("@startuml\n");
    for m in model.modules.values() {
        out.push_str(&format!("component [{}]\n", m.name));
    }
    for (from, found) in dependencies(model, index) {
        for (to, v) in found {
            match v {
                Some(v) => out.push_str(&format!("[{from}] -[#red]-> [{to}] : {}\n", violation_label(&v))),
                None => out.push_str(&format!("[{from}] --> [{to}]\n")),
            }
        }
    }
    out.push_str("@enduml\n");
    out
}

impl Views for ModulithTools {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        let Some(project) = ctx.project() else { return Ok(View::message("Open a project to see its application modules.")) };
        let index = self.indexes.get(&project, COMMAND_BUDGET);
        let complete = index.complete();
        if !complete {
            host::refresh_view(view);
        }
        let model = Model::build(index);
        let refresh = Button::new("refresh", "Refresh").icon("refresh");
        if model.modules.is_empty() {
            let text = if !complete {
                "Scanning…"
            } else if model.roots.is_empty() {
                "No @SpringBootApplication or @Modulithic class found."
            } else {
                "No application modules: the application package has no sub-packages."
            };
            return Ok(View::message(text).button(refresh));
        }
        let dependencies = dependencies(&model, index);
        if view == DIAGRAM_VIEW {
            let (nodes, edges) = diagram_of(&model, index);
            let status = format!("{} modules · {} dependencies", nodes.len(), edges.len());
            return Ok(View::new(Body::Html(diagram::html(&nodes, &edges, "No application modules."))).button(refresh).status(&status));
        }
        let mut items = Vec::new();
        for m in model.modules.values() {
            let mut detail = if m.open { "open".to_owned() } else { "closed".to_owned() };
            if let Some(d) = &m.display_name {
                detail = format!("{detail} · {d}");
            }
            let parent = key(&m.name, "");
            items.push(TreeItem::new(&parent, &m.name).detail(&detail).icon("module").expandable(false));
            for i in m.interface_names() {
                items.push(TreeItem::new(&key(&m.name, &format!("i:{i}")), i).parent(&parent).detail("named interface").icon("interface"));
            }
            for d in m.allowed.iter().flatten() {
                items.push(TreeItem::new(&key(&m.name, &format!("a:{d}")), d).parent(&parent).detail("allowed dependency").icon("module"));
            }
            for (b, v) in dependencies.get(m.name.as_str()).into_iter().flatten() {
                let (detail, icon) = match v {
                    None => ("dependency".to_owned(), "module"),
                    Some(Violation::Internal) => ("dependency on internal types".to_owned(), "warning"),
                    Some(Violation::NotAllowed(d)) => (format!("dependency on {d}, not in allowedDependencies"), "warning"),
                };
                let item = TreeItem::new(&key(b, &format!("d:{}", m.name)), b).parent(&parent).detail(&detail).icon(icon);
                items.push(item);
            }
        }
        self.expansion.apply(&mut items);
        let mut status = format!("{} modules in {}", model.modules.len(), model.roots.join(", "));
        if !complete {
            status.push_str(" · scanning…");
        }
        Ok(View::new(Body::Tree(items)).button(refresh).status(&status))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if self.expansion.handle(&event) {
            host::refresh_view(view);
            return Ok(());
        }

        match event {
            ViewEvent::Button(b) if b == "refresh" => {
                // Resetting a scan in progress would throw away what it has read.
                if let Some(project) = ctx.project()
                    && self.indexes.get(&project, Duration::ZERO).complete()
                {
                    self.indexes.reset();
                }
                host::refresh_view(view);
            }
            ViewEvent::Activate(key) | ViewEvent::Message(key) => {
                let Some(name) = key.strip_prefix("open:").and_then(|k| k.split('#').next()) else { return Ok(()) };
                let Some(project) = ctx.project() else { return Ok(()) };
                let model = Model::build(self.indexes.get(&project, COMMAND_BUDGET));
                if let Some(at) = model.modules.get(name).and_then(|m| m.location.as_ref()) {
                    host::open_location(&at.path, at.range.start.line, at.range.start.character)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl Plugin for ModulithTools {
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        if id != "copy-docs" {
            return Err(Error::NotFound(format!("command {id}")));
        }
        let project = ctx.project().ok_or_else(|| Error::Invalid("Open a project first.".into()))?;
        let index = self.indexes.get(&project, COMMAND_BUDGET);
        let model = Model::build(index);
        if model.modules.is_empty() {
            host::notify(NotificationLevel::Info, TITLE, "No application modules found.");
            return Ok(None);
        }
        match host::copy_to_clipboard(&plantuml(&model, index)) {
            Ok(()) => host::notify(NotificationLevel::Info, TITLE, "PlantUML module diagram copied to the clipboard."),
            Err(Error::Denied(_)) => host::notify(
                NotificationLevel::Warning,
                TITLE,
                "Allow the clipboard for Spring Modulith Tools in Settings › Plugins to copy the module diagram.",
            ),
            Err(e) => return Err(e),
        }
        Ok(None)
    }
}

register_plugin!(ModulithTools, providers, views);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::text::position;

    const VIEW: &str = "modules";

    const SRC: &str = "/project/src/main/java/com/acme/";

    fn setup() -> MockHost {
        let host = MockHost::install();
        let files = [
            ("Application.java", "package com.acme;\n\n@SpringBootApplication\npublic class Application {}\n"),
            ("order/OrderService.java", "package com.acme.order;\n\npublic class OrderService {}\n"),
            ("order/internal/OrderRepository.java", "package com.acme.order.internal;\n\nclass OrderRepository {}\n"),
            (
                "order/package-info.java",
                "@ApplicationModule(displayName = \"Orders\")\npackage com.acme.order;\n\nimport org.springframework.modulith.ApplicationModule;\n",
            ),
            ("inventory/Inventory.java", "package com.acme.inventory;\n\npublic class Inventory {}\n"),
            ("inventory/api/StockApi.java", "package com.acme.inventory.api;\n\npublic interface StockApi {}\n"),
            ("inventory/api/package-info.java", "@org.springframework.modulith.NamedInterface(\"api\")\npackage com.acme.inventory.api;\n"),
            ("inventory/spi/Hook.java", "package com.acme.inventory.spi;\n\n@NamedInterface(\"hooks\")\npublic interface Hook {}\n"),
            (
                "shipping/package-info.java",
                "@ApplicationModule(allowedDependencies = {\"order\", \"inventory :: api\", \"billing\"})\npackage com.acme.shipping;\n",
            ),
            ("shipping/Shipper.java", SHIPPER),
        ];
        for (path, text) in files {
            host.add_file(&format!("src/main/java/com/acme/{path}"), text);
        }
        host.add_file("src/test/java/com/acme/shipping/ShipperTest.java", "package com.acme.shipping;\nimport com.acme.order.internal.OrderRepository;\nclass ShipperTest {}\n");
        host
    }

    const SHIPPER: &str = "package com.acme.shipping;

import java.util.List;
import com.acme.order.OrderService;
import com.acme.order.internal.OrderRepository;
import com.acme.inventory.api.StockApi;
import com.acme.inventory.Inventory;
import com.acme.inventory.spi.Hook;
import com.acme.shipping.internal.Box;

class Shipper {}
";

    fn doc(path: &str, text: &str) -> Document {
        Document { path: format!("{SRC}{path}"), language: "java".into(), version: 1, text: text.into() }
    }

    fn pos(text: &str, needle: &str) -> Position {
        position(text, text.find(needle).unwrap())
    }

    #[test]
    fn reads_package_info() {
        let u = scan("/p/package-info.java", "@ApplicationModule(\n  type = ApplicationModule.Type.OPEN)\npackage a.b;").unwrap();
        let m = u.info.unwrap().module.unwrap();
        assert!(m.open);
        assert_eq!(m.open_fix, None);
        let fix = |text: &str| {
            let u = scan("/p/package-info.java", text).unwrap();
            let e = u.info.unwrap().module.unwrap().open_fix.unwrap();
            let at = offset(text, e.range.start).unwrap();
            format!("{}{}{}", &text[..at], e.text, &text[at..])
        };
        assert_eq!(fix("@ApplicationModule\npackage a.b;"), format!("@ApplicationModule({OPEN})\npackage a.b;"));
        assert_eq!(fix("@ApplicationModule()\npackage a.b;"), format!("@ApplicationModule({OPEN})\npackage a.b;"));
        assert_eq!(fix("@ApplicationModule(displayName = \"B\")\npackage a.b;"), format!("@ApplicationModule(displayName = \"B\", {OPEN})\npackage a.b;"));
    }

    #[test]
    fn warns_about_internal_and_unlisted_dependencies() {
        let host = setup();
        let mut tools = ModulithTools::default();
        assert!(tools.line_markers(&doc("shipping/Shipper.java", SHIPPER)).is_empty());
        let problems = &host.diagnostics()[&format!("{SRC}shipping/Shipper.java")];
        let found: Vec<_> = problems.iter().map(|p| (p.code.as_deref().unwrap(), p.range.start, p.message.as_str())).collect();
        assert_eq!(found, [
            (
                "module-internal",
                pos(SHIPPER, "com.acme.order.internal"),
                "`OrderRepository` is internal to module `order`: use its API package or a named interface."
            ),
            (
                "module-not-allowed",
                pos(SHIPPER, "com.acme.inventory.Inventory"),
                "Module `shipping` doesn't list `inventory` in its allowedDependencies."
            ),
            (
                "module-not-allowed",
                pos(SHIPPER, "com.acme.inventory.spi"),
                "Module `shipping` doesn't list `inventory::hooks` in its allowedDependencies."
            ),
        ]);
        let test = "package com.acme.shipping;\nimport com.acme.order.internal.OrderRepository;\nclass ShipperTest {}\n";
        let test_doc = Document { path: "/project/src/test/java/com/acme/shipping/ShipperTest.java".into(), ..doc("", test) };
        tools.line_markers(&test_doc);
        assert!(!host.diagnostics().contains_key(&test_doc.path));
    }

    #[test]
    fn checks_and_completes_allowed_dependencies() {
        let host = setup();
        let mut tools = ModulithTools::default();
        let text = "@ApplicationModule(allowedDependencies = {\"order\", \"inventory :: api\", \"billing\", \"order::x\", \"inventory::*\"})\npackage com.acme.shipping;\n";
        let d = doc("shipping/package-info.java", text);
        tools.line_markers(&d);
        let problems = &host.diagnostics()[&d.path];
        let found: Vec<_> = problems.iter().map(|p| (p.code.as_deref().unwrap(), p.range.start)).collect();
        assert_eq!(found, [("unknown-module", pos(text, "billing")), ("unknown-module", pos(text, "order::x"))]);

        let labels: Vec<_> = tools.completion(&d, pos(text, "billing"), None).into_iter().map(|i| i.label).collect();
        assert_eq!(labels, ["inventory", "inventory::api", "inventory::hooks", "order"]);
    }

    #[test]
    fn offers_to_open_the_module() {
        let _host = setup();
        let mut tools = ModulithTools::default();
        let d = doc("shipping/Shipper.java", SHIPPER);
        let at = pos(SHIPPER, "order.internal");
        let actions = tools.code_actions(&d, Range::new(at, at));
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Make module `order` open (exposes all its types)");
        let (path, edits) = &actions[0].edits[0];
        assert_eq!(path, &format!("{SRC}order/package-info.java"));
        assert_eq!(edits[0].text, format!(", {OPEN}"));
        let at = pos(SHIPPER, "inventory.Inventory");
        assert!(tools.code_actions(&d, Range::new(at, at)).is_empty());
    }

    #[test]
    fn modules_view_lists_modules_and_opens_them() {
        let host = setup();
        let mut tools = ModulithTools::default();
        let view = tools.render(VIEW, None, &host.invocation()).unwrap();
        let Body::Tree(items) = view.body else { panic!("tree expected") };
        let rows: Vec<_> = items
            .iter()
            .map(|i| (i.parent.is_some(), i.label.as_str(), i.detail.as_deref().unwrap_or_default(), i.icon.as_deref().unwrap()))
            .collect();
        assert_eq!(rows, [
            (false, "inventory", "closed", "module"),
            (true, "api", "named interface", "interface"),
            (true, "hooks", "named interface", "interface"),
            (false, "order", "closed · Orders", "module"),
            (false, "shipping", "closed", "module"),
            (true, "order", "allowed dependency", "module"),
            (true, "inventory :: api", "allowed dependency", "module"),
            (true, "billing", "allowed dependency", "module"),
            (true, "inventory", "dependency on inventory, not in allowedDependencies", "warning"),
            (true, "order", "dependency on internal types", "warning"),
        ]);
        assert_eq!(view.status.as_deref(), Some("3 modules in com.acme"));
        assert!(host.refreshed_views().is_empty(), "the scan is complete");
        let dependency = items.iter().rfind(|i| i.label == "order").unwrap();
        tools.handle(VIEW, ViewEvent::Activate(dependency.key.clone()), &host.invocation()).unwrap();
        assert_eq!(host.opened_locations(), [(format!("{SRC}order/package-info.java"), 0, 0)]);
    }

    #[test]
    fn modules_open_and_close() {
        let host = setup();
        let mut tools = ModulithTools::default();
        let module = |tools: &mut ModulithTools| {
            let view = tools.render(VIEW, None, &host.invocation()).unwrap();
            let Body::Tree(items) = view.body else { panic!("tree expected") };
            items.into_iter().find(|i| i.label == "inventory" && i.parent.is_none()).unwrap()
        };
        let inventory = module(&mut tools);
        assert!(inventory.expandable && !inventory.expanded);

        tools.handle(VIEW, ViewEvent::Expand(inventory.key.clone()), &host.invocation()).unwrap();
        assert_eq!(host.refreshed_views(), [VIEW]);
        assert!(module(&mut tools).expanded);

        tools.handle(VIEW, ViewEvent::Collapse(inventory.key), &host.invocation()).unwrap();
        assert!(!module(&mut tools).expanded);
    }

    #[test]
    fn refresh_keeps_a_scan_in_progress() {
        let host = setup();
        let mut tools = ModulithTools::default();
        let project = host.invocation().project().unwrap();
        tools.indexes.get(&project, Duration::ZERO).update(&format!("{SRC}shipping/Shipper.java"), SHIPPER);
        tools.handle(VIEW, ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
        assert_eq!(tools.indexes.get(&project, Duration::ZERO).files().len(), 1, "not reset");
        assert_eq!(host.refreshed_views(), [VIEW]);

        tools.indexes.get(&project, COMMAND_BUDGET);
        tools.handle(VIEW, ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
        assert!(tools.indexes.get(&project, Duration::ZERO).files().is_empty(), "a complete index starts over");
    }

    #[test]
    fn modulith_diagram_has_expected_nodes_and_edges() {
        let host = setup();
        let mut tools = ModulithTools::default();
        let index = tools.indexes.get(&host.invocation().project().unwrap(), COMMAND_BUDGET);
        let model = Model::build(index);
        let (nodes, edges) = diagram_of(&model, index);
        let nodes: Vec<_> = nodes.iter().map(|n| (n.id.as_str(), n.label.as_str(), n.detail.as_deref())).collect();
        assert_eq!(nodes, [
            ("open:inventory#", "inventory", Some("closed")),
            ("open:order#", "order", Some("closed")),
            ("open:shipping#", "shipping", Some("closed")),
        ]);
        let edges: Vec<_> = edges.iter().map(|e| (e.from.as_str(), e.to.as_str(), e.label.as_deref(), e.warning)).collect();
        assert_eq!(edges, [
            ("open:shipping#", "open:inventory#", Some("inventory not allowed"), true),
            ("open:shipping#", "open:order#", Some("internal types"), true),
        ]);

        let view = tools.render(DIAGRAM_VIEW, None, &host.invocation()).unwrap();
        let Body::Html(page) = view.body else { panic!("html expected") };
        assert!(page.contains("data-id=\"open:order#\""));
        assert_eq!(view.status.as_deref(), Some("3 modules · 2 dependencies"));
        tools.handle(DIAGRAM_VIEW, ViewEvent::Message("open:order#".into()), &host.invocation()).unwrap();
        assert_eq!(host.opened_locations(), [(format!("{SRC}order/package-info.java"), 0, 0)]);

        let empty = MockHost::install();
        let view = ModulithTools::default().render(DIAGRAM_VIEW, None, &empty.invocation()).unwrap();
        assert_eq!(view.body, Body::Message("No @SpringBootApplication or @Modulithic class found.".into()));
    }

    #[test]
    fn modulith_plantuml_text_lists_modules_and_dependencies() {
        let host = setup();
        let mut tools = ModulithTools::default();
        tools.run_command("copy-docs", &host.invocation(), None).unwrap();
        assert_eq!(host.clipboard().as_deref(), Some(PLANTUML));

        let denied = MockHost::install().with_capabilities(&["project.read"]);
        denied.add_file("src/main/java/com/acme/Application.java", "package com.acme;\n@SpringBootApplication\nclass Application {}\n");
        denied.add_file("src/main/java/com/acme/order/Order.java", "package com.acme.order;\nclass Order {}\n");
        ModulithTools::default().run_command("copy-docs", &denied.invocation(), None).unwrap();
        assert_eq!(denied.clipboard(), None);
        assert!(denied.notifications().iter().any(|n| n.body.contains("Allow the clipboard")));
    }

    const PLANTUML: &str = "@startuml
component [inventory]
component [order]
component [shipping]
[shipping] -[#red]-> [inventory] : inventory not allowed
[shipping] -[#red]-> [order] : internal types
@enduml
";
}
