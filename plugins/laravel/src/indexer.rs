//! Project-wide links for the host's document index. A definition is where a
//! file declares a key, a marker is a gutter icon going to the definitions of
//! its key. Keys: `model#Post`, `model-table#posts`, `factory#Post`,
//! `policy#Post`, `observer#Post`, `seeder#Post`, `migration#posts`,
//! `event#Registered`, `listener#Registered`. Routes, views and controllers keep
//! the provider's gutter markers, so nothing here duplicates them.

use crate::{index, php, project_for};
use openidea_plugin::indexing::{Anchor, Document, DocumentIndexer, Edges};
use openidea_plugin::text;
use openidea_plugin::{Position, Range};

use crate::Main;

const MAX_ANCHORS: usize = 4096;
const SKIPPED: &[&str] = &["/vendor/", "/storage/", "/bootstrap/cache/", "/node_modules/"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Role {
    Model,
    Factory,
    Policy,
    Observer,
    Seeder,
    Migration,
    Event,
    Listener,
}

const DIRECTORIES: &[(&str, Role)] = &[
    ("/app/Models/", Role::Model),
    ("/database/factories/", Role::Factory),
    ("/app/Policies/", Role::Policy),
    ("/app/Observers/", Role::Observer),
    ("/database/seeders/", Role::Seeder),
    ("/database/migrations/", Role::Migration),
    ("/app/Events/", Role::Event),
    ("/app/Listeners/", Role::Listener),
];

/// The role of a file, the project root and the file's name without `.php`.
fn classify(path: &str) -> Option<(Role, &str, &str)> {
    let stem = path.rsplit('/').next()?.strip_suffix(".php")?;

    for (directory, role) in DIRECTORIES {
        if let Some(at) = path.find(directory) {
            return Some((*role, &path[..at], stem));
        }
    }

    let (root, rest) = path.rsplit_once("/app/")?;
    (!rest.contains('/')).then_some((Role::Model, root, stem))
}

fn range_of(text: &str, start: usize, length: usize) -> Range {
    let at = text::position(text, start);
    let end = Position::new(at.line, at.character + text[start..start + length].encode_utf16().count() as u32);
    Range::new(at, end)
}

fn class_name(masked: &str, expected: &str) -> Option<usize> {
    masked
        .match_indices("class ")
        .map(|(at, _)| at + "class ".len())
        .find(|start| masked[*start..].starts_with(expected) && masked.as_bytes().get(start + expected.len()).is_none_or(|b| !php::is_identifier(*b)))
}

fn extends_model(masked: &str) -> bool {
    masked.contains(" extends Model") || masked.contains(" extends Authenticatable") || masked.contains(" extends Pivot") || masked.contains("\\Model")
}

fn tables_of_migration(masked: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for call in ["Schema::create(", "Schema::table("] {
        for (at, _) in masked.match_indices(call) {
            let first = php::arguments(masked, at + call.len() - 1).into_iter().next();
            if let Some((table, offset)) = first.and_then(|range| php::literal(masked, range)) {
                found.push((offset, table.to_owned()));
            }
        }
    }
    found
}

/// Short class names of `new X`/`X::dispatch` in an event call, and `X::factory(` receivers.
fn uses_of(masked: &str, marker: &str) -> Vec<(usize, usize)> {
    masked
        .match_indices(marker)
        .filter_map(|(at, _)| {
            let end = at;
            let start = masked[..end].rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).map_or(0, |i| i + 1);
            let name = &masked[start..end];
            (!name.is_empty() && name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && !masked[..start].ends_with("$")).then_some((start, end))
        })
        .collect()
}

fn listener_events(masked: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (at, _) in masked.match_indices("function handle(") {
        let open = at + "function handle".len();
        let Some(first) = php::arguments(masked, open).into_iter().next() else { continue };
        let parameter = masked[first].trim();
        let ty = parameter.split_whitespace().next().unwrap_or_default().trim_start_matches('?');
        if ty.starts_with('$') || ty.is_empty() || matches!(ty, "array" | "string" | "int" | "mixed" | "object") {
            continue;
        }
        found.push(ty.rsplit('\\').next().unwrap_or(ty).to_owned());
    }
    found
}

/// What a file defines and links to. `exists` tells whether a project-relative file exists.
pub fn edges_of(path: &str, text: &str, exists: &dyn Fn(&str) -> bool) -> Edges {
    let mut edges = Edges::default();
    if SKIPPED.iter().any(|segment| path.contains(segment)) {
        return edges;
    }

    let Some((role, _, stem)) = classify(path) else { return edges };
    let masked = php::mask_comments(text);
    let mut definition = |key: String, range: Range| edges.definitions.push(Anchor { key, range });
    let mut markers: Vec<Anchor> = Vec::new();
    let mut reference_anchors: Vec<Anchor> = Vec::new();

    let model_exists = |name: &str| exists(&format!("app/Models/{name}.php")) || exists(&format!("app/{name}.php"));

    match role {
        Role::Model => {
            if !extends_model(&masked) {
                return Edges::default();
            }
            let Some(at) = class_name(&masked, stem) else { return Edges::default() };
            let range = range_of(text, at, stem.len());
            let table = index::explicit_table(&masked).unwrap_or_else(|| index::table_name(stem));

            definition(format!("model#{stem}"), range);
            definition(format!("model-table#{table}"), range);
            markers.push(Anchor { key: format!("migration#{table}"), range });

            for (prefix, directory, suffix) in [("factory", "database/factories", "Factory"), ("policy", "app/Policies", "Policy"), ("observer", "app/Observers", "Observer"), ("seeder", "database/seeders", "Seeder")] {
                if exists(&format!("{directory}/{stem}{suffix}.php")) {
                    markers.push(Anchor { key: format!("{prefix}#{stem}"), range });
                }
            }
        }
        Role::Factory | Role::Policy | Role::Observer | Role::Seeder => {
            let (prefix, suffix) = match role {
                Role::Factory => ("factory", "Factory"),
                Role::Policy => ("policy", "Policy"),
                Role::Observer => ("observer", "Observer"),
                _ => ("seeder", "Seeder"),
            };
            let Some(model) = stem.strip_suffix(suffix).filter(|m| !m.is_empty()) else { return Edges::default() };
            let Some(at) = class_name(&masked, stem) else { return Edges::default() };
            let range = range_of(text, at, stem.len());

            definition(format!("{prefix}#{model}"), range);
            if model_exists(model) {
                markers.push(Anchor { key: format!("model#{model}"), range });
            }
        }
        Role::Migration => {
            for (offset, table) in tables_of_migration(&masked) {
                let range = range_of(text, offset, table.len());
                definition(format!("migration#{table}"), range);
                markers.push(Anchor { key: format!("model-table#{table}"), range });
            }
        }
        Role::Event => {
            let Some(at) = class_name(&masked, stem) else { return Edges::default() };
            let range = range_of(text, at, stem.len());
            definition(format!("event#{stem}"), range);
            markers.push(Anchor { key: format!("listener#{stem}"), range });
        }
        Role::Listener => {
            let Some(at) = class_name(&masked, stem) else { return Edges::default() };
            let range = range_of(text, at, stem.len());
            for event in listener_events(&masked) {
                definition(format!("listener#{event}"), range);
                markers.push(Anchor { key: format!("event#{event}"), range });
            }
        }
    }

    for (start, end) in uses_of(&masked, "::factory(") {
        reference_anchors.push(Anchor { key: format!("factory#{}", &masked[start..end]), range: range_of(text, start, end - start) });
    }

    edges.references = reference_anchors;
    edges.markers = markers;
    edges.definitions.truncate(MAX_ANCHORS);
    edges.references.truncate(MAX_ANCHORS);
    edges.markers.truncate(MAX_ANCHORS);
    edges
}

impl DocumentIndexer for Main {
    fn index(&mut self, doc: &Document) -> Edges {
        let Some((_, root, _)) = classify(&doc.path) else { return Edges::default() };
        let project = project_for(&doc.path);
        let exists = |relative: &str| project.as_ref().is_some_and(|p| p.read_file(&format!("{root}/{relative}")).is_ok());

        edges_of(&doc.path, &doc.text, &exists)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(anchors: &[Anchor]) -> Vec<&str> {
        anchors.iter().map(|a| a.key.as_str()).collect()
    }

    fn everything_exists(_: &str) -> bool {
        true
    }

    fn nothing_exists(_: &str) -> bool {
        false
    }

    #[test]
    fn model_defines_itself_and_links_to_the_files_that_exist() {
        let text = "<?php\nnamespace App\\Models;\nclass Post extends Model\n{\n}\n";
        let edges = edges_of("/p/app/Models/Post.php", text, &everything_exists);
        assert_eq!(keys(&edges.definitions), ["model#Post", "model-table#posts"]);
        assert_eq!(keys(&edges.markers), ["migration#posts", "factory#Post", "policy#Post", "observer#Post", "seeder#Post"]);
        assert_eq!(edges.definitions[0].range.start.line, 2);

        let alone = edges_of("/p/app/Models/Post.php", text, &nothing_exists);
        assert_eq!(keys(&alone.markers), ["migration#posts"]);
    }

    #[test]
    fn explicit_table_and_non_model_classes() {
        let edges = edges_of("/p/app/Models/Article.php", "<?php\nclass Article extends Model\n{\n    protected $table = 'posts';\n}\n", &nothing_exists);
        assert_eq!(keys(&edges.definitions), ["model#Article", "model-table#posts"]);
        assert!(edges_of("/p/app/Models/Helper.php", "<?php\nclass Helper\n{\n}\n", &nothing_exists).definitions.is_empty());
    }

    #[test]
    fn counterparts_link_back_to_the_model_only_when_it_exists() {
        let factory = "<?php\nclass PostFactory extends Factory\n{\n}\n";
        let edges = edges_of("/p/database/factories/PostFactory.php", factory, &everything_exists);
        assert_eq!(keys(&edges.definitions), ["factory#Post"]);
        assert_eq!(keys(&edges.markers), ["model#Post"]);
        assert!(edges_of("/p/database/factories/PostFactory.php", factory, &nothing_exists).markers.is_empty());

        let policy = edges_of("/p/app/Policies/PostPolicy.php", "<?php\nclass PostPolicy\n{\n}\n", &everything_exists);
        assert_eq!((keys(&policy.definitions), keys(&policy.markers)), (vec!["policy#Post"], vec!["model#Post"]));
    }

    #[test]
    fn migrations_define_their_tables_and_point_at_models() {
        let text = "<?php\nreturn new class extends Migration {\n    public function up(): void\n    {\n        Schema::create('posts', function (Blueprint $t) {});\n        Schema::table('users', function (Blueprint $t) {});\n    }\n};\n";
        let edges = edges_of("/p/database/migrations/2024_01_01_create_posts_table.php", text, &nothing_exists);
        assert_eq!(keys(&edges.definitions), ["migration#posts", "migration#users"]);
        assert_eq!(keys(&edges.markers), ["model-table#posts", "model-table#users"]);
        assert_eq!(edges.definitions[0].range.start.line, 4);
    }

    #[test]
    fn events_and_listeners_meet_through_the_handle_type() {
        let event = edges_of("/p/app/Events/OrderShipped.php", "<?php\nclass OrderShipped\n{\n}\n", &nothing_exists);
        assert_eq!((keys(&event.definitions), keys(&event.markers)), (vec!["event#OrderShipped"], vec!["listener#OrderShipped"]));

        let listener = "<?php\nuse App\\Events\\OrderShipped;\nclass SendNotice\n{\n    public function handle(OrderShipped $event): void {}\n}\n";
        let edges = edges_of("/p/app/Listeners/SendNotice.php", listener, &nothing_exists);
        assert_eq!((keys(&edges.definitions), keys(&edges.markers)), (vec!["listener#OrderShipped"], vec!["event#OrderShipped"]));

        let scalar = "<?php\nclass A\n{\n    public function handle($x) {}\n}\n";
        assert!(edges_of("/p/app/Listeners/A.php", scalar, &nothing_exists).definitions.is_empty());
    }

    #[test]
    fn factory_calls_are_references_and_vendor_files_publish_nothing() {
        let text = "<?php\nclass Seed\n{\n    public function run() { Post::factory()->create(); $x->factory(); }\n}\n";
        let edges = edges_of("/p/app/Models/Seed.php", "<?php\nclass Seed extends Model\n{\n    function a() { return Post::factory(); }\n}\n", &nothing_exists);
        assert_eq!(keys(&edges.references), ["factory#Post"]);
        assert!(edges_of("/p/vendor/a/app/Models/B.php", text, &nothing_exists).definitions.is_empty());
        assert!(edges_of("/p/app/Models/Multi.php", "<?php\nclass Multi extends Model\n{\n}\n", &nothing_exists).references.is_empty());
    }

    #[test]
    fn multibyte_and_crlf_keep_ranges_in_utf16() {
        let text = "<?php\r\n// é 😀\r\nclass Post extends Model\r\n{\r\n}\r\n";
        let edges = edges_of("/p/app/Models/Post.php", text, &nothing_exists);
        let range = edges.definitions[0].range;
        assert_eq!((range.start.line, range.start.character, range.end.character), (2, 6, 10));
    }
}
