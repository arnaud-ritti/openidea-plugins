//! A text index of a Laravel project: named routes, views, config keys,
//! translation keys, env keys, middleware aliases, assets, controllers and
//! their methods, each with where it is defined.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use openidea_plugin::Position;
use openidea_plugin::host::{self, Project};

use crate::refs::{self, Kind};
use crate::{blade, controllers, migrations, php, routes};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub detail: Option<String>,
    /// Absolute path; empty for built-in names.
    pub path: String,
    pub at: Position,
    /// Length in UTF-16 units of the name in the file; 0 to point at the file.
    pub width: u32,
}

pub type Names = BTreeMap<String, Entry>;

pub trait AddName {
    fn add(&mut self, name: &str, detail: Option<String>, path: &str, at: Position, width: usize);
}

impl AddName for Names {
    /// The first definition of a name wins.
    fn add(&mut self, name: &str, detail: Option<String>, path: &str, at: Position, width: usize) {
        if name.is_empty() {
            return;
        }

        self.entry(name.to_owned()).or_insert(Entry { detail, path: path.to_owned(), at, width: width as u32 });
    }
}

/// A route of `routes/`, with where it is defined.
#[derive(Clone, Debug)]
pub struct RouteEntry {
    pub def: routes::RouteDef,
    pub path: String,
    pub at: Position,
}

/// A controller rendering a view: `return view('posts.index')`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Render {
    pub view: String,
    /// `PostController::index`.
    pub caller: String,
    pub path: String,
    pub at: Position,
}

#[derive(Default, Debug)]
pub struct Index {
    pub routes: Names,
    pub route_defs: Vec<RouteEntry>,
    pub views: Names,
    pub blade: BTreeMap<String, blade::Facts>,
    pub config: Names,
    pub translations: Names,
    /// `lang/<locale>/<file>.php` files by the file's key, main locale first.
    pub translation_files: BTreeMap<String, Vec<String>>,
    pub env: Names,
    pub middleware: Names,
    pub assets: Names,
    pub controllers: Names,
    /// Public methods by controller class.
    pub methods: BTreeMap<String, Names>,
    /// Blade components by tag name (`forms.input` for `<x-forms.input>`).
    pub components: Names,
    /// Livewire components by tag name (`counter` for `<livewire:counter>`).
    pub livewire: Names,
    /// Files `@vite()` can name, relative to the project.
    pub vite: Names,
    /// Gate abilities (`Gate::define`) and policy methods.
    pub abilities: Names,
    /// Validation rule names.
    pub rules: Names,
    /// Columns by table, from the migrations.
    pub columns: BTreeMap<String, Names>,
    /// Table of each model, by short class name and by file.
    pub models: BTreeMap<String, String>,
    pub model_files: BTreeMap<String, String>,
    pub renders: Vec<Render>,
    /// Inertia pages by name (`Posts/Index`), from `resources/js/Pages` and `resources/js/pages`.
    pub inertia_pages: Names,
    /// File names without extension under `database/factories`.
    pub factories: BTreeSet<String>,
    /// Models that name their factory themselves (`newFactory`, `UseFactory`).
    pub factory_overrides: BTreeSet<String>,
    /// A provider registers component directories or namespaces the index can't follow.
    pub open_components: bool,
    /// Some files were skipped to stay within the call's time budget.
    pub partial: bool,
}

// ponytail: 3 s of reading per build, then the index is partial; incremental per-file updates if projects outgrow it.
const BUDGET: Duration = Duration::from_secs(3);
const MAX_FILES: u32 = 5000;
const VIEW_ROOTS: &[&str] = &["resources/views", "app/views", "resources/templates"];
const LANG_ROOTS: &[&str] = &["lang", "resources/lang"];
const VITE_EXTENSIONS: &[&str] = &["css", "scss", "sass", "less", "js", "jsx", "ts", "tsx", "vue"];
const INERTIA_PAGE_ROOTS: &[&str] = &["resources/js/Pages", "resources/js/pages"];
const INERTIA_PAGE_EXTENSIONS: &[&str] = &["vue", "jsx", "tsx", "svelte"];
const ROUTE_FILES: &[&str] = &["routes/**/*.php", "app/Http/routes.php", "app/routes.php"];

/// Rules of Laravel's validator (the "Available Validation Rules" of the documentation).
const VALIDATION_RULES: &[&str] = &[
    "accepted", "accepted_if", "active_url", "after", "after_or_equal", "alpha", "alpha_dash", "alpha_num", "array", "ascii", "bail",
    "before", "before_or_equal", "between", "boolean", "confirmed", "contains", "current_password", "date", "date_equals",
    "date_format", "decimal", "declined", "declined_if", "different", "digits", "digits_between", "dimensions", "distinct",
    "doesnt_end_with", "doesnt_start_with", "email", "ends_with", "enum", "exclude", "exclude_if", "exclude_unless", "exclude_with",
    "exclude_without", "exists", "extensions", "file", "filled", "gt", "gte", "hex_color", "image", "in", "in_array", "integer", "ip",
    "ipv4", "ipv6", "json", "list", "lowercase", "lt", "lte", "mac_address", "max", "max_digits", "mimetypes", "mimes", "min",
    "min_digits", "missing", "missing_if", "missing_unless", "missing_with", "missing_with_all", "multiple_of", "not_in", "not_regex",
    "nullable", "numeric", "present", "present_if", "present_unless", "present_with", "present_with_all", "prohibited", "prohibited_if",
    "prohibited_unless", "prohibits", "regex", "required", "required_array_keys", "required_if", "required_if_accepted",
    "required_if_declined", "required_unless", "required_with", "required_with_all", "required_without", "required_without_all", "same",
    "size", "sometimes", "starts_with", "string", "timezone", "ulid", "unique", "uppercase", "url", "uuid",
];

/// Route middleware the framework registers (Laravel's `Middleware::defaultAliases()` and groups).
const BUILT_IN_MIDDLEWARE: &[&str] = &[
    "api",
    "auth",
    "auth.basic",
    "auth.session",
    "cache.headers",
    "can",
    "guest",
    "password.confirm",
    "precognitive",
    "signed",
    "throttle",
    "verified",
    "web",
];

pub fn is_laravel(project: &Project) -> bool {
    if project.read_file("artisan").is_ok() {
        return true;
    }

    project.read_file("composer.json").is_ok_and(|composer| composer.contains("\"laravel/framework\""))
}

/// Byte offset → editor position, for many offsets of one file.
pub struct Lines<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    pub fn new(text: &'a str) -> Self {
        let starts = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
        Self { text, starts }
    }

    pub fn position(&self, at: usize) -> Position {
        let line = self.starts.partition_point(|&start| start <= at) - 1;
        let start = self.starts[line];
        let character = self.text[start..at].chars().map(|c| c.len_utf16() as u32).sum();
        Position::new(line as u32, character)
    }
}

/// `resources/views/posts/index.blade.php` relative to its root: `posts.index`;
/// `vendor/mail/x.blade.php` overrides the view `mail::x`.
pub fn view_name(relative: &str) -> Option<String> {
    let stem = relative.strip_suffix(".blade.php").or_else(|| relative.strip_suffix(".php"))?;

    if let Some(rest) = stem.strip_prefix("vendor/")
        && let Some((package, view)) = rest.split_once('/')
    {
        return Some(format!("{package}::{}", view.replace('/', ".")));
    }

    Some(stem.replace('/', "."))
}

/// `Forms/InputField.php` is the tag name `forms.input-field`.
fn tag_name(relative: &str) -> String {
    let stem = relative.strip_suffix(".php").unwrap_or(relative);
    stem.split('/').map(kebab).collect::<Vec<_>>().join(".")
}

fn kebab(word: &str) -> String {
    separated(word, '-')
}

/// `UserProfile` as `user-profile` (or `user_profile`).
pub fn separated(word: &str, separator: char) -> String {
    let mut out = String::new();
    let mut previous: Option<char> = None;

    for c in word.chars() {
        if c.is_ascii_uppercase() && previous.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit()) {
            out.push(separator);
        }
        out.push(c.to_ascii_lowercase());
        previous = Some(c);
    }

    out
}

/// The table Eloquent uses for a model without `$table`: snake case, plural.
// ponytail: regular English plurals only (-y, -s/x/z/ch/sh, else -s); set `$table` for irregular names.
pub fn table_name(class: &str) -> String {
    let snake = separated(class, '_');

    if let Some(stem) = snake.strip_suffix('y')
        && !stem.ends_with(['a', 'e', 'i', 'o', 'u'])
    {
        return format!("{stem}ies");
    }

    if snake.ends_with(['s', 'x', 'z']) || snake.ends_with("ch") || snake.ends_with("sh") {
        return format!("{snake}es");
    }

    format!("{snake}s")
}

/// `protected $table = 'x';`
fn explicit_table(text: &str) -> Option<String> {
    let at = text.find("$table")?;
    let rest = text[at + "$table".len()..].trim_start().strip_prefix('=')?;
    let start = text.len() - rest.len();
    php::literal(text, start..start + rest.find(';')?).map(|(table, _)| table.to_owned())
}

fn comma_separated(key: &str) -> Vec<String> {
    let value = host::setting_string(key).unwrap_or_default();
    value.split(',').map(|p| p.trim().trim_matches('/')).filter(|p| !p.is_empty()).map(str::to_owned).collect()
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl Index {
    pub fn build(project: &Project) -> Index {
        let started = Instant::now();
        let mut index = Index::default();
        let root = project.root().unwrap_or_default();
        let relative = |path: &str| path.strip_prefix(&format!("{root}/")).unwrap_or(path).to_owned();

        let files = |globs: &[String]| -> Vec<String> {
            let mut paths: Vec<String> = globs.iter().flat_map(|glob| project.find_files(glob, MAX_FILES).unwrap_or_default()).collect();
            paths.sort();
            paths.dedup();
            paths
        };
        let globs = |patterns: &[&str]| -> Vec<String> { patterns.iter().map(|p| (*p).to_owned()).collect() };

        let read = |path: &str, index: &mut Index| -> Option<String> {
            if started.elapsed() > BUDGET {
                index.partial = true;
                return None;
            }
            project.read_file(path).ok()
        };

        let mut view_roots: Vec<String> = VIEW_ROOTS.iter().map(|r| (*r).to_owned()).collect();
        view_roots.extend(comma_separated("viewPaths"));
        for view_root in &view_roots {
            for path in files(&[format!("{view_root}/**/*.php")]) {
                let Some(name) = relative(&path).strip_prefix(&format!("{view_root}/")).and_then(view_name) else { continue };
                index.views.add(&name, None, &path, Position::new(0, 0), 0);

                if !path.ends_with(".blade.php") {
                    continue;
                }

                if let Some(text) = read(&path, &mut index) {
                    index.blade.entry(name).or_insert_with(|| blade::facts(&text));
                }
            }
        }

        for path in files(&globs(ROUTE_FILES)) {
            let Some(text) = read(&path, &mut index) else { continue };
            let masked = php::mask_comments(&text);
            let lines = Lines::new(&text);

            for def in routes::parse(&masked) {
                let at = lines.position(def.offset);
                if let Some(name) = &def.name {
                    let detail = format!("{} {} → {}", def.methods, def.uri, def.action);
                    index.routes.add(name, Some(detail), &path, at, def.width);
                }
                index.route_defs.push(RouteEntry { def, path: path.clone(), at });
            }

            index.renders.extend(renders(&path, &text));
        }

        for path in files(&globs(&["app/Http/Controllers/**/*.php"])) {
            let Some(text) = read(&path, &mut index) else { continue };
            let relative_path = relative(&path);
            let inside = relative_path.trim_start_matches("app/Http/Controllers/");
            let masked = php::mask_comments(&text);
            let lines = Lines::new(&text);

            if let Some(controller) = controllers::parse(inside, &masked) {
                let mut methods = Names::new();
                for (method, offset) in &controller.methods {
                    methods.add(method, None, &path, lines.position(*offset), method.len());
                }

                let class_length = controller.class.rsplit('\\').next().map_or(0, str::len);
                index.controllers.add(&controller.class, None, &path, lines.position(controller.class_offset), class_length);
                index.methods.insert(controller.class, methods);
            }

            index.renders.extend(renders(&path, &text));
        }

        for path in files(&globs(&["app/Mail/**/*.php"])) {
            if let Some(text) = read(&path, &mut index) {
                index.renders.extend(renders(&path, &text));
            }
        }

        for path in files(&globs(&["config/**/*.php"])) {
            let Some(text) = read(&path, &mut index) else { continue };
            let relative_path = relative(&path);
            let Some(file_key) = relative_path.strip_prefix("config/").and_then(|p| p.strip_suffix(".php")).map(|p| p.replace('/', ".")) else {
                continue;
            };

            index.config.add(&file_key, Some("config file".into()), &path, Position::new(0, 0), 0);
            let masked = php::mask_comments(&text);
            let Some(open) = php::returned_array(&masked) else { continue };

            let lines = Lines::new(&text);
            let mut leaves = Vec::new();
            php::array_paths(&masked, open, &file_key, &mut leaves, 0);
            for leaf in leaves {
                let detail = if leaf.array { Some("array".to_owned()) } else { leaf.value };
                index.config.add(&leaf.path, detail, &path, lines.position(leaf.offset), leaf.width);
            }
        }

        index.index_translations(&relative, &files, &read);

        for file in [".env.example", ".env"] {
            let path = format!("{root}/{file}");
            if let Some(text) = read(&path, &mut index) {
                env_keys(&path, &text, &mut index.env);
            }
        }

        for file in ["bootstrap/app.php", "app/Http/Kernel.php"] {
            let path = format!("{root}/{file}");
            if let Some(text) = read(&path, &mut index) {
                middleware_aliases(&path, &text, &mut index.middleware);
            }
        }
        for name in BUILT_IN_MIDDLEWARE {
            index.middleware.add(name, Some("framework".into()), "", Position::new(0, 0), 0);
        }

        let public = host::setting_string("publicDirectory").filter(|d| !d.is_empty()).unwrap_or_else(|| "public".into());
        let public = public.trim_matches('/').to_owned();
        for path in files(&[format!("{public}/**/*")]) {
            let name = relative(&path).trim_start_matches(&format!("{public}/")).to_owned();
            if !name.ends_with(".php") {
                index.assets.add(&name, None, &path, Position::new(0, 0), 0);
            }
        }

        index.components = Names::new();
        for path in files(&globs(&["app/View/Components/**/*.php"])) {
            let Some(text) = read(&path, &mut index) else { continue };
            let relative_path = relative(&path);
            let inside = relative_path.trim_start_matches("app/View/Components/");
            let lines = Lines::new(&text);
            if let Some(class) = controllers::parse(inside, &php::mask_comments(&text)) {
                let short = class.class.rsplit('\\').next().map_or(0, str::len);
                index.components.add(&tag_name(inside), None, &path, lines.position(class.class_offset), short);
            }
        }
        // Class components win over anonymous ones of the same name.
        for path in files(&globs(&["resources/views/components/**/*.blade.php"])) {
            let relative_path = relative(&path);
            let inside = relative_path.trim_start_matches("resources/views/components/").trim_end_matches(".blade.php");
            let name = inside.replace('/', ".");
            let name = name.strip_suffix(".index").unwrap_or(&name);
            index.components.add(name, None, &path, Position::new(0, 0), 0);

            if let Some((parent, last)) = name.rsplit_once('.') {
                if parent.rsplit('.').next() == Some(last) {
                    index.components.add(parent, None, &path, Position::new(0, 0), 0);
                }
            }
        }

        for directory in ["app/Livewire", "app/Http/Livewire"] {
            for path in files(&[format!("{directory}/**/*.php")]) {
                let Some(text) = read(&path, &mut index) else { continue };
                let relative_path = relative(&path);
                let inside = relative_path.trim_start_matches(&format!("{directory}/"));
                let lines = Lines::new(&text);
                if let Some(class) = controllers::parse(inside, &php::mask_comments(&text)) {
                    let short = class.class.rsplit('\\').next().map_or(0, str::len);
                    index.livewire.add(&tag_name(inside), None, &path, lines.position(class.class_offset), short);
                }
            }
        }

        for root in INERTIA_PAGE_ROOTS {
            for extension in INERTIA_PAGE_EXTENSIONS {
                for path in files(&[format!("{root}/**/*.{extension}")]) {
                    let name = relative(&path).trim_start_matches(&format!("{root}/")).trim_end_matches(&format!(".{extension}")).to_owned();
                    index.inertia_pages.add(&name, None, &path, Position::new(0, 0), 0);
                }
            }
        }

        for path in files(&globs(&["database/factories/**/*.php"])) {
            if let Some(stem) = file_name(&path).strip_suffix(".php") {
                index.factories.insert(stem.to_owned());
            }
        }

        for extension in VITE_EXTENSIONS {
            for path in files(&[format!("resources/**/*.{extension}")]) {
                let name = relative(&path);
                if !name.starts_with("resources/views/") {
                    index.vite.add(&name, None, &path, Position::new(0, 0), 0);
                }
            }
        }

        for path in files(&globs(&["app/Providers/**/*.php"])) {
            let Some(text) = read(&path, &mut index) else { continue };
            let masked = php::mask_comments(&text);
            let lines = Lines::new(&text);
            index.open_components |= ["anonymousComponentPath(", "anonymousComponentNamespace(", "componentNamespace(", "Blade::components("]
                .iter()
                .any(|call| masked.contains(call));
            for (at, _) in masked.match_indices("Blade::component(") {
                let aliases = php::arguments(&masked, at + "Blade::component".len());
                for (alias, offset) in aliases.into_iter().take(2).filter_map(|range| php::literal(&masked, range)) {
                    index.components.add(alias, Some("Blade::component".into()), &path, lines.position(offset), alias.len());
                }
            }
            for (at, _) in masked.match_indices("Gate::define(") {
                let first = php::arguments(&masked, at + "Gate::define".len()).into_iter().next();
                if let Some((ability, offset)) = first.and_then(|range| php::literal(&masked, range)) {
                    index.abilities.add(ability, Some("Gate::define".into()), &path, lines.position(offset), ability.len());
                }
            }
        }

        for path in files(&globs(&["app/Policies/**/*.php"])) {
            let Some(text) = read(&path, &mut index) else { continue };
            let relative_path = relative(&path);
            let inside = relative_path.trim_start_matches("app/Policies/");
            let lines = Lines::new(&text);
            let Some(policy) = controllers::parse(inside, &php::mask_comments(&text)) else { continue };
            for (method, offset) in policy.methods.iter().filter(|(m, _)| !matches!(m.as_str(), "before" | "after")) {
                index.abilities.add(method, Some(policy.class.clone()), &path, lines.position(*offset), method.len());
            }
        }

        for path in files(&globs(&["database/migrations/**/*.php"])) {
            let Some(text) = read(&path, &mut index) else { continue };
            let masked = php::mask_comments(&text);
            let lines = Lines::new(&text);
            for column in migrations::parse(&masked) {
                let names = index.columns.entry(column.table).or_default();
                names.add(&column.name, Some(column.kind), &path, lines.position(column.offset), column.name.len());
            }
        }

        for path in files(&globs(&["app/Models/**/*.php", "app/*.php"])) {
            let Some(text) = read(&path, &mut index) else { continue };
            let relative_path = relative(&path);
            let inside = relative_path.trim_start_matches("app/").trim_start_matches("Models/");
            let masked = php::mask_comments(&text);
            let Some(model) = controllers::parse(inside, &masked) else { continue };

            let short = model.class.rsplit('\\').next().unwrap_or(&model.class).to_owned();
            let table = explicit_table(&masked).unwrap_or_else(|| table_name(&short));
            if masked.contains("newFactory") || masked.contains("UseFactory") {
                index.factory_overrides.insert(short.clone());
            }
            index.model_files.insert(path, table.clone());
            index.models.insert(short, table);
        }

        for rule in VALIDATION_RULES {
            index.rules.add(rule, Some("validation rule".into()), "", Position::new(0, 0), 0);
        }

        index.renders.sort_by(|a, b| (&a.path, a.at.line, a.at.character).cmp(&(&b.path, b.at.line, b.at.character)));
        index
    }

    /// Keys of `lang/<locale>/<file>.php` (as `file.key`) and `lang/<locale>.json`.
    fn index_translations(
        &mut self,
        relative: &dyn Fn(&str) -> String,
        files: &dyn Fn(&[String]) -> Vec<String>,
        read: &dyn Fn(&str, &mut Index) -> Option<String>,
    ) {
        let main = host::setting_string("locale").filter(|l| !l.is_empty()).unwrap_or_else(|| "en".into());

        let mut found: Vec<(String, String, bool, String)> = Vec::new();
        for lang_root in LANG_ROOTS {
            for path in files(&[format!("{lang_root}/**/*.php"), format!("{lang_root}/**/*.json")]) {
                let relative_path = relative(&path);
                let Some(inside) = relative_path.strip_prefix(&format!("{lang_root}/")) else { continue };
                let parts: Vec<&str> = inside.split('/').collect();

                // `vendor/<package>/<locale>/<file>.php`, `<locale>/<file>.php`, `<locale>.json`
                let (namespace, rest) = match parts.as_slice() {
                    ["vendor", package, rest @ ..] => (format!("{package}::"), rest.to_vec()),
                    rest => (String::new(), rest.to_vec()),
                };
                let (locale, key) = match rest.as_slice() {
                    [json] if json.ends_with(".json") => (json.trim_end_matches(".json").to_owned(), String::new()),
                    [locale, file @ ..] if !file.is_empty() => (
                        (*locale).to_owned(),
                        file.join("/").trim_end_matches(".php").to_owned(),
                    ),
                    _ => continue,
                };

                let json = path.ends_with(".json");
                found.push((locale, format!("{namespace}{key}"), json, path));
            }
        }
        found.sort_by_key(|(locale, _, _, path)| (*locale != main, locale.clone(), path.clone()));

        for (locale, key, json, path) in found {
            let Some(text) = read(&path, self) else { continue };
            let lines = Lines::new(&text);

            if json {
                for (name, offset) in json_keys(&text) {
                    let value = format!("[{locale}]");
                    self.translations.add(&name, Some(value), &path, lines.position(offset), name.len());
                }
                continue;
            }

            let masked = php::mask_comments(&text);
            let Some(open) = php::returned_array(&masked) else { continue };
            self.translation_files.entry(key.clone()).or_default().push(path.clone());

            let mut leaves = Vec::new();
            php::array_paths(&masked, open, &key, &mut leaves, 0);
            for leaf in leaves {
                let detail = leaf.value.map(|v| format!("{v} [{locale}]"));
                self.translations.add(&leaf.path, detail, &path, lines.position(leaf.offset), leaf.width);
            }
        }
    }

    /// The view named by a view file.
    pub fn view_of(&self, path: &str) -> Option<&str> {
        self.views.iter().find(|(_, entry)| entry.path == path).map(|(name, _)| name.as_str())
    }

    /// The views a view extends, nearest first.
    pub fn parents<'a>(&'a self, view: &'a str) -> Vec<&'a str> {
        let mut parents = Vec::new();
        let mut current = view;

        // Layouts are a few levels deep; the limit stops cycles.
        for _ in 0..10 {
            let Some(parent) = self.blade.get(current).and_then(|f| f.extends.as_deref()) else { break };
            if parents.contains(&parent) {
                break;
            }
            parents.push(parent);
            current = parent;
        }

        parents
    }

    /// Views that extend or include `view`.
    pub fn users_of(&self, view: &str) -> Vec<&str> {
        let uses = |facts: &blade::Facts| facts.extends.as_deref() == Some(view) || facts.includes.iter().any(|i| i == view);
        self.blade.iter().filter(|(_, facts)| uses(facts)).map(|(name, _)| name.as_str()).collect()
    }

    /// The public methods of a controller named by its class or short name.
    pub fn controller_methods(&self, class: &str) -> Option<&Names> {
        let short = class.rsplit('\\').next().unwrap_or(class);
        self.methods.iter().find(|(known, _)| known.rsplit('\\').next() == Some(short)).map(|(_, methods)| methods)
    }
}

/// View renders of a PHP file: `view('x')`, `->view('x')`, `Route::view`.
fn renders(path: &str, text: &str) -> Vec<Render> {
    let lines = Lines::new(text);
    let stem = file_name(path).trim_end_matches(".php");

    refs::references("php", text)
        .into_iter()
        .filter(|r| r.kind == Kind::View && r.closed)
        .map(|reference| {
            let method = text[..reference.start].rfind("function ").map(|f| {
                let name = &text[f + "function ".len()..];
                &name[..name.bytes().position(|b| !php::is_identifier(b)).unwrap_or(name.len())]
            });
            let caller = match method.filter(|m| !m.is_empty()) {
                Some(method) => format!("{stem}::{method}"),
                None => stem.to_owned(),
            };

            Render { view: text[reference.start..reference.end].to_owned(), caller, path: path.to_owned(), at: lines.position(reference.start) }
        })
        .collect()
}

/// `KEY=value` lines of an env file; values are not read.
fn env_keys(path: &str, text: &str, names: &mut Names) {
    for (line_number, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let key_part = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        let Some((key, _)) = key_part.split_once('=') else { continue };
        let key = key.trim();

        let is_key = !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.');
        if is_key && !trimmed.starts_with('#') {
            let column = (line.len() - key_part.len()) as u32;
            names.add(key, Some(file_name(path).to_owned()), path, Position::new(line_number as u32, column), key.len());
        }
    }
}

/// Aliases registered in `bootstrap/app.php` (`$middleware->alias([...])`)
/// or `app/Http/Kernel.php` (`$routeMiddleware`, `$middlewareAliases`).
fn middleware_aliases(path: &str, text: &str, names: &mut Names) {
    let masked = php::mask_comments(text);
    let lines = Lines::new(text);
    let mut arrays = Vec::new();

    for (at, _) in masked.match_indices("alias(") {
        let open = at + "alias".len();
        arrays.extend(php::arguments(&masked, open).into_iter().next().and_then(|first| php::array_open(&masked, first)));
    }

    for property in ["$routeMiddleware", "$middlewareAliases"] {
        for (at, _) in masked.match_indices(property) {
            let rest = masked[at + property.len()..].trim_start();
            if let Some(after) = rest.strip_prefix('=') {
                let start = masked.len() - after.len();
                arrays.extend(php::array_open(&masked, start..masked.len()));
            }
        }
    }

    for open in arrays {
        for item in php::items(&masked, open) {
            let Some((alias, offset)) = item.key.and_then(|k| php::literal(&masked, k)) else { continue };
            let class = php::class_constant(&masked, item.value.clone()).map(str::to_owned);
            names.add(alias, class, path, lines.position(offset), alias.len());
        }
    }
}

/// `"key": "value"` pairs of a flat JSON translation file, with the offset of each key.
fn json_keys(text: &str) -> Vec<(String, usize)> {
    let bytes = text.as_bytes();
    let mut keys = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }

        let end = php::string_end(bytes, i);
        let after = text[end..].trim_start();
        let is_key = after.starts_with(':') && end > i + 1;

        if is_key && bytes[end - 1] == b'"' {
            let key = text[i + 1..end - 1].replace("\\\"", "\"").replace("\\/", "/");
            keys.push((key, i + 1));
            // Skip the value, which may itself be a string.
            let value = after[1..].trim_start();
            let value_start = text.len() - value.len();
            i = if value.starts_with('"') { php::string_end(bytes, value_start) } else { value_start };
            continue;
        }

        i = end.max(i + 1);
    }

    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_views() {
        assert_eq!(view_name("posts/index.blade.php").as_deref(), Some("posts.index"));
        assert_eq!(view_name("welcome.php").as_deref(), Some("welcome"));
        assert_eq!(view_name("vendor/mail/html/button.blade.php").as_deref(), Some("mail::html.button"));
        assert_eq!(view_name("readme.md"), None);
    }

    #[test]
    fn names_component_tags() {
        assert_eq!(tag_name("Alert.php"), "alert");
        assert_eq!(tag_name("Forms/InputField.php"), "forms.input-field");
        assert_eq!(tag_name("Counter2Button.php"), "counter2-button");
    }

    #[test]
    fn names_model_tables() {
        assert_eq!(table_name("User"), "users");
        assert_eq!(table_name("UserProfile"), "user_profiles");
        assert_eq!(table_name("Category"), "categories");
        assert_eq!(table_name("Day"), "days");
        assert_eq!(table_name("Address"), "addresses");
        assert_eq!(explicit_table("class A { protected $table = 'legacy_users'; }").as_deref(), Some("legacy_users"));
        assert_eq!(explicit_table("class A { public $timestamps = false; }"), None);
    }

    #[test]
    fn reads_env_files() {
        let mut names = Names::new();
        env_keys("/p/.env.example", "# c\nAPP_NAME=Laravel\nexport DB_HOST=x\nnot a key=1\n  QUEUE=sync\n", &mut names);
        let found: Vec<_> = names.iter().map(|(k, e)| (k.as_str(), e.at.line, e.at.character)).collect();
        assert_eq!(found, [("APP_NAME", 1, 0), ("DB_HOST", 2, 7), ("QUEUE", 4, 2)]);
    }

    #[test]
    fn reads_middleware_aliases() {
        let mut names = Names::new();
        let app = "->withMiddleware(function (Middleware $middleware) {\n    $middleware->alias([\n        'admin' => AdminMiddleware::class,\n        'role' => \\Spatie\\Role::class,\n    ]);\n})";
        middleware_aliases("/p/bootstrap/app.php", app, &mut names);
        middleware_aliases("/p/app/Http/Kernel.php", "protected $routeMiddleware = [\n 'auth' => \\App\\Http\\Middleware\\Authenticate::class,\n];", &mut names);
        let found: Vec<_> = names.iter().map(|(k, e)| (k.as_str(), e.detail.as_deref())).collect();
        assert_eq!(
            found,
            [
                ("admin", Some("AdminMiddleware")),
                ("auth", Some("App\\Http\\Middleware\\Authenticate")),
                ("role", Some("Spatie\\Role"))
            ]
        );
    }

    #[test]
    fn reads_json_translation_keys() {
        let text = "{\n  \"Welcome\": \"Bienvenue\",\n  \"Say \\\"hi\\\"\": \"Dis \\\"salut\\\"\",\n  \"Key: value\": \"x\"\n}";
        let keys: Vec<_> = json_keys(text).into_iter().map(|(k, at)| (k, text[at..].chars().next().unwrap())).collect();
        assert_eq!(keys, [("Welcome".into(), 'W'), ("Say \"hi\"".into(), 'S'), ("Key: value".into(), 'K')]);
    }

    #[test]
    fn finds_renders_with_their_callers() {
        let text = "<?php\nclass PostController {\n    public function index()\n    {\n        return view('posts.index');\n    }\n}\n";
        let found = renders("/p/app/Http/Controllers/PostController.php", text);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].view.as_str(), found[0].caller.as_str(), found[0].at), ("posts.index", "PostController::index", Position::new(4, 21)));
    }
}
