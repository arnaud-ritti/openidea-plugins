//! Go to Declaration between a configuration key and the Java code that declares it, in both
//! directions: `application.properties|yaml` keys and `@ConfigProperty` / `@ConfigMapping` members.
//! Keys are the written names without profile, matched as written (Quarkus does not relax names).

use crate::checks::normalize;
use crate::java::declared_keys;
use crate::{QuarkusTools, bundled, is_quarkus_config};
use openidea_plugin::indexing::{Anchor, Document, DocumentIndexer, Edges};
use spring_text::config;
use spring_text::metadata::{Lookup, strip_profile};
use spring_text::range;

const MAX_ANCHORS: usize = 4096;

fn in_config(key: &str) -> String {
    format!("cfg#{key}")
}

fn in_java(key: &str) -> String {
    format!("java#{key}")
}

fn config_edges(path: &str, text: &str) -> Edges {
    let lookup = Lookup(vec![bundled()]);
    let mut edges = Edges::default();
    let all = config::keys(path, text);
    for k in &all {
        let parent = all.iter().any(|o| o.key.len() > k.key.len() && o.key.starts_with(&k.key) && matches!(o.key.as_bytes()[k.key.len()], b'.' | b'['));
        if parent {
            continue;
        }
        let key = normalize(&lookup, strip_profile(&k.key)).into_owned();
        if key.is_empty() || key.contains(['"', '[']) {
            continue;
        }
        let at = range(text, k.at, k.end);
        edges.definitions.push(Anchor { key: in_config(&key), range: at });
        edges.references.push(Anchor { key: in_java(&key), range: at });
        edges.markers.push(Anchor { key: in_java(&key), range: at });
    }
    edges
}

fn java_edges(text: &str) -> Edges {
    let mut edges = Edges::default();
    for (key, at, end) in declared_keys(text) {
        let at = range(text, at, end);
        edges.definitions.push(Anchor { key: in_java(&key), range: at });
        edges.references.push(Anchor { key: in_config(&key), range: at });
        edges.markers.push(Anchor { key: in_config(&key), range: at });
    }
    edges
}

impl DocumentIndexer for QuarkusTools {
    fn index(&mut self, doc: &Document) -> Edges {
        let mut edges = if doc.path.ends_with(".java") {
            java_edges(&doc.text)
        } else if is_quarkus_config(&doc.path) && self.uses_quarkus(&doc.path) {
            config_edges(&doc.path, &doc.text)
        } else {
            Edges::default()
        };
        edges.definitions.truncate(MAX_ANCHORS);
        edges.references.truncate(MAX_ANCHORS);
        edges.markers.truncate(MAX_ANCHORS);
        edges
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    fn index(path: &str, text: &str) -> Edges {
        QuarkusTools::default().index(&Document { path: path.into(), text: text.into(), symbols: Vec::new() })
    }

    fn keys(anchors: &[Anchor]) -> Vec<&str> {
        anchors.iter().map(|a| a.key.as_str()).collect()
    }

    fn quarkus_host() -> MockHost {
        let host = MockHost::install();
        host.add_file("pom.xml", "<project><groupId>io.quarkus</groupId></project>");
        host
    }

    #[test]
    fn java_declarations_point_at_config_keys_and_back() {
        let java = "@ConfigMapping(prefix = \"server\")\ninterface S { String hostName(); Map<String, String> labels(); }\nclass G { @ConfigProperty(name = \"greeting.message\") String m; @ConfigProperty(name = \"a\" + B) String x; }\n";
        let edges = index("/project/src/main/java/G.java", java);
        assert_eq!(keys(&edges.definitions), ["java#greeting.message", "java#server.host-name", "java#server.labels"]);
        assert_eq!(keys(&edges.references), ["cfg#greeting.message", "cfg#server.host-name", "cfg#server.labels"]);
        assert_eq!(keys(&edges.markers), keys(&edges.references));
        let message = &edges.definitions[0].range;
        assert_eq!((message.start.line, message.start.character, message.end.character), (2, 34, 50));
        assert!(index("/project/src/main/java/N.java", "class N {}").definitions.is_empty());
    }

    #[test]
    fn config_keys_point_at_java_and_back_in_quarkus_modules_only() {
        let _host = quarkus_host();
        let text = "greeting.message=hi\n%dev.server.host-name=x\nquarkus.datasource.\"a\".jdbc.url=u\nquarkus.log.category.\"x\".level=1\n";
        let edges = index("/project/src/main/resources/application.properties", text);
        assert_eq!(keys(&edges.definitions), ["cfg#greeting.message", "cfg#server.host-name", "cfg#quarkus.datasource.jdbc.url"]);
        assert_eq!(keys(&edges.references), ["java#greeting.message", "java#server.host-name", "java#quarkus.datasource.jdbc.url"]);
        let yaml = index("/project/src/main/resources/application.yaml", "greeting:\n  message: hi\n");
        assert_eq!(keys(&yaml.definitions), ["cfg#greeting.message"]);
        assert!(index("/project/src/main/resources/messages.properties", text).definitions.is_empty());
        let spring = MockHost::install();
        spring.add_file("pom.xml", "<project>spring-boot</project>");
        assert!(index("/project/src/main/resources/application.properties", text).definitions.is_empty());
    }
}
