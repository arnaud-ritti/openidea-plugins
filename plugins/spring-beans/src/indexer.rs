//! Project-wide bean navigation for the host's document index: the model and
//! the string sites of a file, published as keys (`bean#name`, `type#Simple`).

use crate::lib_hits;
use crate::model::{self, FileModel};
use crate::sites::Site;
use crate::Navigator;
use openidea_plugin::indexing::{Alias, Anchor, Document, DocumentIndexer, Edges};
use spring_text::java;
use spring_text::range;

const MAX_ANCHORS: usize = 4096;
const MAX_ALIASES: usize = 256;
const LISTENER_ANNOTATIONS: [&str; 2] = ["EventListener", "TransactionalEventListener"];

fn bean_key(name: &str) -> String {
    format!("bean#{name}")
}

fn type_key(ty: &str) -> String {
    format!("type:{ty}#bean")
}

fn listener_key(event: &str) -> String {
    format!("event:{event}#listener")
}

fn publisher_key(event: &str) -> String {
    format!("publish:{event}#call")
}

fn class_key(ty: &str) -> String {
    format!("class:{ty}#decl")
}

fn xml_key(ty: &str) -> String {
    format!("xml:{ty}#bean")
}

fn definitions(model: &FileModel) -> Vec<Anchor> {
    let mut out = Vec::new();
    for bean in &model.beans {
        let names = std::iter::once(&bean.name).chain(&bean.aliases).chain(&bean.qualifiers);
        out.extend(names.filter(|n| !n.is_empty()).map(|n| Anchor { key: bean_key(n), range: bean.range }));
        if bean.ty.is_empty() {
            continue;
        }
        out.push(Anchor { key: type_key(&bean.ty), range: bean.range });
        if bean.kind.starts_with('<') {
            out.push(Anchor { key: xml_key(&bean.ty), range: bean.range });
        }
    }
    out.extend(model.types.iter().map(|t| Anchor { key: class_key(&t.name), range: t.range }));
    out
}

/// Lookups of a supertype, of a published event and of a listened-to event follow the
/// project's hierarchy across files, which per-file definitions cannot.
fn aliases(model: &FileModel) -> Vec<Alias> {
    let pairs = model.types.iter().flat_map(|t| t.supers.iter().map(move |s| (t.name.as_str(), s.as_str())));
    pairs
        .flat_map(|(sub, sup)| {
            [
                Alias { prefix: format!("type:{sup}#"), target: format!("type:{sub}#") },
                Alias { prefix: format!("event:{sub}#"), target: format!("event:{sup}#") },
                Alias { prefix: format!("publish:{sup}#"), target: format!("publish:{sub}#") },
            ]
        })
        .take(MAX_ALIASES)
        .collect()
}

/// Markers from classes to the XML beans of that class, and from those beans back to the class.
fn class_xml_markers(model: &FileModel) -> Vec<Anchor> {
    let classes = model.types.iter().map(|t| Anchor { key: xml_key(&t.name), range: t.range });
    let xml_beans = model.beans.iter().filter(|b| b.kind.starts_with('<') && !b.ty.is_empty());
    classes.chain(xml_beans.map(|b| Anchor { key: class_key(&b.ty), range: b.range })).collect()
}

fn class_literals(text: &str) -> impl Iterator<Item = &str> {
    let word = |c: char| !(c.is_alphanumeric() || c == '_' || c == '.' || c == '$');
    text.split(word).filter_map(|w| w.strip_suffix(".class")).filter_map(|w| w.rsplit('.').next())
}

/// Event publishers (`publishEvent(new X(...))`) and listeners, as (definitions, markers):
/// a publisher's marker goes to the listeners of its event, a listener's to the publishers.
fn events(path: &str, text: &str) -> (Vec<Anchor>, Vec<Anchor>) {
    let (mut definitions, mut markers) = (Vec::new(), Vec::new());
    if !path.ends_with(".java") {
        return (definitions, markers);
    }
    let file = java::parse(text);
    let mut listen = |event: &str, range| {
        definitions.push(Anchor { key: listener_key(event), range });
        markers.push(Anchor { key: publisher_key(event), range });
    };
    for ty in &file.types {
        for method in ty.methods() {
            let Some(annotation) = method.annotations.iter().find(|a| LISTENER_ANNOTATIONS.contains(&a.name.as_str())) else { continue };
            let declared: Vec<&str> =
                annotation.args.iter().filter(|a| a.name == "value" || a.name == "classes").flat_map(|a| class_literals(&a.text)).collect();
            let from_parameter = method.params.first().map(|p| p.ty.name.as_str());
            let events = if declared.is_empty() { from_parameter.into_iter().collect() } else { declared };
            let at = range(text, method.at, method.at + method.name.len());
            events.into_iter().for_each(|event| listen(event, at));
        }
        let at = range(text, ty.at, ty.at + ty.name.len());
        for event in ty.implements.iter().filter(|i| i.name == "ApplicationListener").filter_map(|i| i.args.first()) {
            listen(&event.name, at);
        }
    }
    for call in java::calls(text).iter().filter(|c| c.name == "publishEvent") {
        let Some(created) = call.args.first().and_then(|a| a.text.strip_prefix("new ")) else { continue };
        let name: String = created.trim_start().chars().take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '$')).collect();
        let Some(event) = name.rsplit('.').next().filter(|e| !e.is_empty()) else { continue };
        let at = range(text, call.at, call.at + call.name.len());
        markers.push(Anchor { key: listener_key(event), range: at });
        definitions.push(Anchor { key: publisher_key(event), range: at });
    }
    (definitions, markers)
}

fn injections(model: &FileModel) -> Vec<Anchor> {
    model
        .injections
        .iter()
        .filter(|i| !i.ty.is_empty() && i.qualifier.is_none())
        .map(|i| Anchor { key: type_key(&i.ty), range: i.range })
        .collect()
}

fn named_references(path: &str, text: &str) -> Vec<Anchor> {
    let is_xml = path.ends_with(".xml");
    let mut out = Vec::new();
    for hit in lib_hits(path, text) {
        if hit.site != Site::BeanRef {
            continue;
        }
        let Some(value) = text.get(hit.from..hit.to) else { continue };
        if value.contains("${") || value.contains("#{") {
            continue;
        }
        let names: Vec<(usize, &str)> = if is_xml { model::name_tokens(value).collect() } else { vec![(0, value)] };
        for (at, name) in names.into_iter().filter(|(_, n)| !n.is_empty()) {
            out.push(Anchor { key: bean_key(name), range: range(text, hit.from + at, hit.from + at + name.len()) });
        }
    }
    out
}

impl DocumentIndexer for Navigator {
    fn index(&mut self, doc: &Document) -> Edges {
        let model = model::parse(&doc.path, &doc.text).unwrap_or_default();
        let mut references = named_references(&doc.path, &doc.text);
        let markers = injections(&model);
        references.extend(markers.iter().cloned());
        let mut definitions = definitions(&model);
        let (event_definitions, event_markers) = events(&doc.path, &doc.text);
        definitions.extend(event_definitions);
        let mut markers = markers;
        markers.extend(event_markers);
        markers.extend(class_xml_markers(&model));
        definitions.truncate(MAX_ANCHORS);
        references.truncate(MAX_ANCHORS);
        markers.truncate(MAX_ANCHORS);
        Edges { definitions, references, aliases: aliases(&model), markers }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(path: &str, text: &str) -> Edges {
        Navigator::default().index(&Document { path: path.into(), text: text.into(), symbols: Vec::new() })
    }

    fn keys(anchors: &[Anchor]) -> Vec<&str> {
        anchors.iter().map(|a| a.key.as_str()).collect()
    }

    const SERVICE: &str = "@Service(\"users\")\nclass UserServiceImpl implements UserService {\n    UserServiceImpl(UserRepository repo, @Qualifier(\"sms\") Notifier n) {}\n}\n";

    #[test]
    fn stereotype_class_defines_its_name_and_types() {
        let edges = index("A.java", SERVICE);
        let defined = keys(&edges.definitions);
        assert!(defined.contains(&"bean#users"));
        assert!(defined.contains(&"type:UserServiceImpl#bean"));
        assert!(!defined.contains(&"type:UserService#bean"));
        assert!(edges.aliases.iter().any(|a| a.prefix == "type:UserService#" && a.target == "type:UserServiceImpl#"));
    }

    #[test]
    fn injection_points_and_qualifiers_are_references() {
        let edges = index("A.java", SERVICE);
        let referenced = keys(&edges.references);
        assert!(referenced.contains(&"type:UserRepository#bean"));
        assert!(referenced.contains(&"bean#sms"));
        assert!(!referenced.contains(&"type:Notifier#bean"));
        assert_eq!(keys(&edges.markers), ["type:UserRepository#bean", "xml:UserServiceImpl#bean"]);
    }

    #[test]
    fn bean_method_defines_by_method_name() {
        let edges = index("C.java", "@Configuration\nclass C {\n    @Bean\n    Clock clock() { return null; }\n}\n");
        assert!(keys(&edges.definitions).contains(&"bean#clock"));
        assert!(keys(&edges.definitions).contains(&"type:Clock#bean"));
    }

    #[test]
    fn xml_beans_and_reference_lists() {
        let xml = "<beans>\n  <bean id=\"a\" class=\"com.acme.A\" depends-on=\"b, c\">\n    <property name=\"x\" ref=\"d\"/>\n  </bean>\n</beans>\n";
        let edges = index("beans.xml", xml);
        assert!(keys(&edges.definitions).contains(&"bean#a"));
        let referenced = keys(&edges.references);
        for name in ["bean#b", "bean#c", "bean#d"] {
            assert!(referenced.contains(&name), "{name}");
        }
        let b = edges.references.iter().find(|r| r.key == "bean#c").unwrap();
        assert_eq!((b.range.start.line, b.range.start.character), (1, 49));
    }

    #[test]
    fn unrelated_file_publishes_nothing() {
        let edges = index("Plain.java", "class Plain { int x; }\n");
        assert!(edges.references.is_empty() && edges.aliases.is_empty());
        assert_eq!(keys(&edges.definitions), ["class:Plain#decl"]);
    }

    const PUBLISHER: &str = "class Orders {\n    void place() {\n        events.publishEvent(new OrderPlaced(this));\n    }\n}\n";
    const LISTENERS: &str = "class Mails implements ApplicationListener<OrderShipped> {\n    @EventListener\n    void on(OrderPlaced e) {}\n    @TransactionalEventListener(classes = {OrderCancelled.class, Other.class})\n    void gone() {}\n}\n";

    #[test]
    fn publisher_marker_goes_to_listeners_and_back() {
        let published = index("Orders.java", PUBLISHER);
        assert_eq!(keys(&published.markers), ["event:OrderPlaced#listener", "xml:Orders#bean"]);
        assert_eq!(keys(&published.definitions), ["class:Orders#decl", "publish:OrderPlaced#call"]);
        assert_eq!(published.markers[0].range.start.line, 2);

        let listening = index("Mails.java", LISTENERS);
        let defined = keys(&listening.definitions);
        for key in ["event:OrderPlaced#listener", "event:OrderCancelled#listener", "event:Other#listener", "event:OrderShipped#listener"] {
            assert!(defined.contains(&key), "{key}");
        }
        assert!(keys(&listening.markers).contains(&"publish:OrderPlaced#call"));
    }

    #[test]
    fn events_in_kotlin_and_unknown_payloads_are_ignored() {
        assert!(!keys(&index("A.kt", "class A { fun f() { publisher.publishEvent(Foo()) } }").markers).iter().any(|k| k.starts_with("event:")));
        let edges = index("B.java", "class B { void f(Object o) { events.publishEvent(o); events.publishEvent(\"x\"); } }");
        assert!(!keys(&edges.markers).iter().any(|k| k.starts_with("event:")));
    }

    #[test]
    fn subtypes_alias_in_each_direction_of_their_lookup() {
        let edges = index("Admin.java", "class Admin extends User {}\n");
        let pairs: Vec<(&str, &str)> = edges.aliases.iter().map(|a| (a.prefix.as_str(), a.target.as_str())).collect();
        assert!(pairs.contains(&("type:User#", "type:Admin#")));
        assert!(pairs.contains(&("event:Admin#", "event:User#")));
        assert!(pairs.contains(&("publish:User#", "publish:Admin#")));
    }

    #[test]
    fn classes_and_their_xml_beans_mark_each_other() {
        let java = index("A.java", "class A {}\n");
        assert_eq!(keys(&java.markers), ["xml:A#bean"]);

        let xml = index("beans.xml", "<beans>\n  <bean id=\"a\" class=\"com.acme.A\"/>\n  <bean id=\"b\" class=\"com.acme.A\"/>\n</beans>\n");
        assert_eq!(keys(&xml.markers), ["class:A#decl", "class:A#decl"]);
        assert_eq!(keys(&xml.definitions).iter().filter(|k| **k == "xml:A#bean").count(), 2);
    }
}
