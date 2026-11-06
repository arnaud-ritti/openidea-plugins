use super::*;
use openidea_plugin::mock::MockHost;

fn laravel_project(host: &MockHost) {
    host.add_file("composer.json", r#"{"require": {"laravel/framework": "^11.0"}}"#);
    host.add_file(
        "routes/web.php",
        "<?php\nuse App\\Http\\Controllers\\PostController;\n\nRoute::get('/', fn () => view('welcome'))->name('home');\nRoute::resource('posts', PostController::class)->only(['index', 'show']);\nRoute::prefix('admin')->name('admin.')->group(function () {\n    Route::get('/dashboard', [PostController::class, 'index'])->name('dashboard');\n});\n",
    );
    host.add_file(
        "app/Http/Controllers/PostController.php",
        "<?php\nnamespace App\\Http\\Controllers;\n\nclass PostController extends Controller\n{\n    public function index()\n    {\n        return view('posts.index');\n    }\n\n    public function show($id) {}\n\n    protected function hidden() {}\n}\n",
    );
    host.add_file("resources/views/welcome.blade.php", "<h1>Welcome</h1>\n");
    host.add_file("resources/views/layouts/app.blade.php", "<html>\n@yield('content')\n@stack('scripts')\n</html>\n");
    host.add_file(
        "resources/views/posts/index.blade.php",
        "@extends('layouts.app')\n@section('content')\n    @include('partials.nav')\n@endsection\n@push('scripts')\n@endpush\n",
    );
    host.add_file("resources/views/partials/nav.blade.php", "<nav>{{ route('home') }}</nav>\n");
    host.add_file("config/app.php", "<?php\n\nreturn [\n    'name' => env('APP_NAME', 'Laravel'),\n    'mail' => [\n        'from' => 'a@b.c',\n    ],\n    'providers' => [\n        'A',\n    ],\n];\n");
    host.add_file("lang/en/messages.php", "<?php\n\nreturn [\n    'welcome' => 'Welcome!',\n    'nested' => [\n        'deep' => 'Deep',\n    ],\n];\n");
    host.add_file("lang/fr/messages.php", "<?php\n\nreturn [\n    'welcome' => 'Bienvenue',\n    'only_fr' => 'Seulement',\n];\n");
    host.add_file("lang/en.json", "{\n    \"Sign in\": \"Sign in\"\n}\n");
    host.add_file(".env.example", "APP_NAME=Laravel\nDB_HOST=127.0.0.1\n");
    host.add_file("bootstrap/app.php", "<?php\nreturn Application::configure()->withMiddleware(function (Middleware $middleware) {\n    $middleware->alias(['admin' => AdminOnly::class]);\n})->create();\n");
    host.add_file("public/css/app.css", "");
    host.add_file("app/View/Components/Alert.php", "<?php\nnamespace App\\View\\Components;\n\nclass Alert extends Component\n{\n}\n");
    host.add_file("app/View/Components/Forms/InputField.php", "<?php\nclass InputField extends Component {}\n");
    host.add_file("resources/views/components/card.blade.php", "<div>{{ $slot }}</div>\n");
    host.add_file("resources/views/components/alert.blade.php", "<div class=\"alert\"></div>\n");
    host.add_file("resources/views/components/panel/index.blade.php", "<section></section>\n");
    host.add_file("app/Livewire/Counter.php", "<?php\nclass Counter extends Component {}\n");
    host.add_file(
        "database/migrations/2024_01_01_create_posts_table.php",
        "<?php\nreturn new class extends Migration {\n    public function up(): void\n    {\n        Schema::create('posts', function (Blueprint $table) {\n            $table->id();\n            $table->string('title');\n            $table->text('body');\n            $table->timestamps();\n        });\n        Schema::create('users', function (Blueprint $table) {\n            $table->string('email');\n        });\n    }\n};\n",
    );
    host.add_file("app/Models/Post.php", "<?php\nclass Post extends Model\n{\n    protected $fillable = ['title'];\n}\n");
    host.add_file("app/Models/Article.php", "<?php\nclass Article extends Model\n{\n    protected $table = 'posts';\n}\n");
    host.add_file("resources/js/app.js", "");
    host.add_file("resources/css/app.css", "");
    host.add_file(
        "app/Providers/AuthServiceProvider.php",
        "<?php\nclass AuthServiceProvider\n{\n    public function boot()\n    {\n        Gate::define('view-reports', fn ($user) => true);\n    }\n}\n",
    );
    host.add_file(
        "app/Policies/PostPolicy.php",
        "<?php\nclass PostPolicy\n{\n    public function before($user) {}\n    public function update($user, $post) {}\n    protected function helper() {}\n}\n",
    );
}

fn activated(host: &MockHost) -> Main {
    laravel_project(host);
    let mut plugin = Main::default();
    plugin.activate(&host.context("aritti.laravel")).unwrap();
    plugin
}

fn doc(path: &str, language: &str, text: &str) -> Document {
    Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
}

fn labels(items: Vec<CompletionItem>) -> Vec<String> {
    items.into_iter().map(|item| item.label).collect()
}

/// The text without its `|` and the position the `|` marked.
fn cursor(marked: &str) -> (String, Position) {
    let marker = if marked.contains('^') { '^' } else { '|' };
    let at = marked.find(marker).unwrap();
    let text = marked.replacen(marker, "", 1);
    let position = text::position(&text, at);
    (text, position)
}

fn host_text(host: &MockHost, path: &str) -> String {
    host.invocation().project().unwrap().read_file(path).unwrap()
}

#[test]
fn indexes_the_project() {
    let host = MockHost::install();
    let plugin = activated(&host);
    let index = plugin.index_for("/project/x").unwrap();

    let routes: Vec<_> = index.routes.keys().map(String::as_str).collect();
    assert_eq!(routes, ["admin.dashboard", "home", "posts.index", "posts.show"]);
    assert_eq!(index.routes["posts.show"].detail.as_deref(), Some("GET|HEAD /posts/{post} → PostController@show"));

    let views: Vec<_> = index.views.keys().map(String::as_str).collect();
    assert_eq!(views, ["components.alert", "components.card", "components.panel.index", "layouts.app", "partials.nav", "posts.index", "welcome"]);

    assert!(index.config.contains_key("app.mail.from") && index.config.contains_key("app"));
    assert_eq!(index.translations["messages.welcome"].detail.as_deref(), Some("Welcome! [en]"));
    assert!(index.translations.contains_key("messages.only_fr") && index.translations.contains_key("Sign in"));
    assert_eq!(index.methods["PostController"].keys().map(String::as_str).collect::<Vec<_>>(), ["index", "show"]);
    assert_eq!(index.middleware["admin"].detail.as_deref(), Some("AdminOnly"));
    assert!(index.env.contains_key("DB_HOST") && index.assets.contains_key("css/app.css"));
}

#[test]
fn completes_and_navigates_from_the_index() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let mut complete = |marked: &str| {
        let (text, at) = cursor(marked);
        labels(plugin.completion(&doc("/project/app/Http/Controllers/HomeController.php", "php", &text), at, None))
    };

    assert_eq!(complete("<?php route('|');"), ["admin.dashboard", "home", "posts.index", "posts.show"]);
    assert_eq!(complete("<?php view('|');"), ["components.alert", "components.card", "components.panel.index", "layouts.app", "partials.nav", "posts.index", "welcome"]);
    assert!(complete("<?php config('app.|');").contains(&"app.mail.from".to_owned()));
    assert!(complete("<?php __('messages.|');").contains(&"messages.nested.deep".to_owned()));
    assert_eq!(complete("<?php env('|');"), ["APP_NAME", "DB_HOST"]);
    assert_eq!(complete("<?php asset('|');"), ["css/app.css"]);
    assert!(complete("<?php Route::get('/x', 'P')->middleware('|');").contains(&"admin".to_owned()));
    assert!(complete("<?php Route::get('/x', 'P')->middleware('|');").contains(&"throttle".to_owned()));

    let usage = "<?php\nredirect()->route('home');\nview('posts.index');\nconfig('app.name');\n__('messages.welcome');\n";
    let usage = doc("/project/app/Http/Controllers/X.php", "php", usage);
    let route = plugin.definition(&usage, Position::new(1, 20));
    let routes = host_text(&host, "routes/web.php");
    let name_at = text::position(&routes, routes.find("'home'").unwrap() + 1);
    assert_eq!(route[0].path, "/project/routes/web.php");
    assert_eq!(route[0].range, Range::new(name_at, Position::new(name_at.line, name_at.character + 4)));
    assert_eq!(plugin.definition(&usage, Position::new(2, 10))[0].path, "/project/resources/views/posts/index.blade.php");
    assert_eq!(plugin.definition(&usage, Position::new(3, 10))[0].path, "/project/config/app.php");
    let translation = plugin.definition(&usage, Position::new(4, 12));
    assert_eq!(translation[0].path, "/project/lang/en/messages.php");
}

#[test]
fn completes_controller_actions() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let mut complete = |marked: &str| {
        let (text, at) = cursor(marked);
        labels(plugin.completion(&doc("/project/routes/api.php", "php", &text), at, None))
    };

    assert_eq!(complete("<?php Route::get('/a', 'Post|');"), ["PostController@index", "PostController@show"]);
    assert_eq!(complete("<?php Route::get('/b', [PostController::class, '|']);"), ["index", "show"]);
    assert_eq!(complete("<?php Route::resource('p', 'Post|');"), ["PostController"]);

    let method = plugin.definition(&doc("/project/routes/api.php", "php", "<?php\nRoute::get('/a', 'PostController@show');\n"), Position::new(1, 30));
    let controller = host_text(&host, "app/Http/Controllers/PostController.php");
    let show_at = text::position(&controller, controller.find("show(").unwrap());
    assert_eq!(method[0].path, "/project/app/Http/Controllers/PostController.php");
    assert_eq!(method[0].range.start, show_at);
}

#[test]
fn completes_blade_directives_and_layout_names() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let mut complete = |marked: &str| {
        let (text, at) = cursor(marked);
        labels(plugin.completion(&doc("/project/resources/views/posts/show.blade.php", "blade", &text), at, None))
    };

    let views = ["components.alert", "components.card", "components.panel.index", "layouts.app", "partials.nav", "posts.index", "welcome"];
    assert_eq!(complete("@extends('|')"), views);
    assert_eq!(complete("@extends('layouts.app')\n@section('|')"), ["content"]);
    assert_eq!(complete("@extends('layouts.app')\n@push('|')"), ["scripts"]);
    assert_eq!(complete("@include('|')"), views);
    assert_eq!(complete("<a href=\"{{ route('|') }}\">").len(), 4);
    assert!(complete("<a>{{ __('messages.w|') }}</a>").contains(&"messages.welcome".to_owned()));

    let (text, at) = cursor("@extends('layouts.app')\n@section('con|tent')\n");
    let section = plugin.definition(&doc("/project/resources/views/posts/show.blade.php", "blade", &text), at);
    let layout = host_text(&host, "resources/views/layouts/app.blade.php");
    let yield_at = text::position(&layout, layout.find("content").unwrap());
    assert_eq!(section[0].path, "/project/resources/views/layouts/app.blade.php");
    assert_eq!(section[0].range.start, yield_at);
}

#[test]
fn completes_and_navigates_components_vite_abilities_and_rules() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let mut complete = |language: &str, marked: &str| {
        let (text, at) = cursor(marked);
        let path = if language == "blade" { "/project/resources/views/a.blade.php" } else { "/project/app/Http/Requests/R.php" };
        labels(plugin.completion(&doc(path, language, &text), at, None))
    };

    assert_eq!(complete("blade", "<x-|"), ["alert", "card", "forms.input-field", "panel"]);
    assert_eq!(complete("blade", "<livewire:|"), ["counter"]);
    assert_eq!(complete("blade", "@vite(['|'])"), ["resources/css/app.css", "resources/js/app.js"]);
    assert_eq!(complete("php", "<?php $user->can('|');"), ["update", "view-reports"]);
    assert!(complete("php", "<?php $request->validate(['name' => 'required|ma^']);").contains(&"max".to_owned()));
    assert!(!complete("php", "<?php $request->validate(['name' => 'required|^']);").is_empty());

    let rules = complete("php", "<?php $request->validate(['name' => 'required|ma^x:255']);");
    assert!(rules.contains(&"max".to_owned()) && rules.contains(&"mimes".to_owned()));

    let mut go = |marked: &str| {
        let (text, at) = cursor(marked);
        plugin.definition(&doc("/project/resources/views/a.blade.php", "blade", &text), at)
    };
    let alert = go("<x-al^ert />");
    assert_eq!(alert[0].path, "/project/app/View/Components/Alert.php");
    assert_eq!(alert[0].range.start, Position::new(3, 6));
    assert_eq!(go("<x-forms.in^put-field />")[0].path, "/project/app/View/Components/Forms/InputField.php");
    assert_eq!(go("<x-ca^rd />")[0].path, "/project/resources/views/components/card.blade.php");
    assert_eq!(go("<x-pan^el />")[0].path, "/project/resources/views/components/panel/index.blade.php");
    assert_eq!(go("<livewire:coun^ter />")[0].path, "/project/app/Livewire/Counter.php");

    let ability = doc("/project/app/Http/Controllers/X.php", "php", "<?php\n$user->can('view-reports');\n$user->can('update');\n");
    assert_eq!(plugin.definition(&ability, Position::new(1, 15))[0].path, "/project/app/Providers/AuthServiceProvider.php");
    assert_eq!(plugin.definition(&ability, Position::new(2, 15))[0].path, "/project/app/Policies/PostPolicy.php");
}

#[test]
fn completes_columns_from_migrations() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let mut complete = |path: &str, marked: &str| {
        let (text, at) = cursor(marked);
        labels(plugin.completion(&doc(path, "php", &text), at, None))
    };
    let columns = ["body", "created_at", "id", "title", "updated_at"];
    let controller = "/project/app/Http/Controllers/X.php";

    assert_eq!(complete(controller, "<?php Post::where('|');"), columns);
    assert_eq!(complete(controller, "<?php Post::query()->with('author')->orderBy('|');"), columns);
    assert_eq!(complete(controller, "<?php Article::latest('|');"), columns);
    assert_eq!(complete(controller, "<?php DB::table('users')->where('|');"), ["email"]);
    assert_eq!(complete("/project/app/Models/Post.php", "<?php class Post { protected $fillable = ['|']; }"), columns);
    assert_eq!(complete("/project/app/Models/Post.php", "<?php class Post { protected $casts = ['|' => 'int']; }"), columns);
    assert_eq!(complete("/project/app/Models/Post.php", "<?php class Post { function scopeA($query) { return $query->where('|'); } }"), columns);
    assert!(complete(controller, "<?php Widget::where('|');").is_empty());
    assert!(complete(controller, "<?php Route::get('/x', 'A@b')->where('|');").is_empty());
    assert!(complete(controller, "<?php $query->where('|');").is_empty());

    let usage = doc(controller, "php", "<?php\nPost::where('title', 'x');\n");
    let migration = host_text(&host, "database/migrations/2024_01_01_create_posts_table.php");
    let title_at = text::position(&migration, migration.find("title").unwrap());
    let found = plugin.definition(&usage, Position::new(1, 14));
    assert_eq!(found[0].path, "/project/database/migrations/2024_01_01_create_posts_table.php");
    assert_eq!(found[0].range.start, title_at);
}

#[test]
fn reads_blade_files_the_ide_calls_php() {
    let host = MockHost::install();
    let mut plugin = activated(&host);

    let blade = doc("/project/resources/views/a.blade.php", "php", "@include('welcome')\n");
    assert_eq!(plugin.definition(&blade, Position::new(0, 13))[0].path, "/project/resources/views/welcome.blade.php");
}

#[test]
fn reports_missing_names_but_not_unknown_ones() {
    let host = MockHost::install();
    let plugin = activated(&host);
    let index = plugin.index_for("/project/x").unwrap();

    let php = "<?php\nview('nope'); view('welcome'); view('mail::button'); view('a.' . $b);\nconfig('app.gone'); config('app.mail.from'); config('services.stripe.key'); config('app.mail.gone');\n__('messages.gone'); __('messages.welcome'); __('vendor::x.y'); __('Plain sentence'); __('validation.required');\nroute('missing');\n";
    let found: Vec<_> = problems(index, "php", php, false).into_iter().map(|d| (d.code.unwrap(), d.range.start.line)).collect();
    assert_eq!(found, [("missing-view".into(), 1), ("missing-config".into(), 2), ("missing-config".into(), 2), ("missing-translation".into(), 3)]);

    let with_routes: Vec<_> = problems(index, "php", php, true).into_iter().map(|d| d.code.unwrap()).collect();
    assert!(with_routes.contains(&"missing-route".to_owned()));
}

#[test]
fn publishes_problems_of_opened_files() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    host.add_file("resources/views/broken.blade.php", "@include('missing.partial')\n");
    plugin.handle_events(&[Event::FileOpened("/project/resources/views/broken.blade.php".into())], &host.invocation());
    assert_eq!(host.diagnostics()["/project/resources/views/broken.blade.php"].len(), 1);
}

#[test]
fn marks_routes_views_and_layouts() {
    let host = MockHost::install();
    let mut plugin = activated(&host);

    let controller = host_text(&host, "app/Http/Controllers/PostController.php");
    let markers = plugin.line_markers(&doc("/project/app/Http/Controllers/PostController.php", "php", &controller));
    let tooltips: Vec<_> = markers.iter().map(|m| (m.line, m.tooltip.as_str())).collect();
    assert_eq!(tooltips, [(7, "View posts.index"), (5, "Route GET|HEAD /posts"), (10, "Route GET|HEAD /posts/{post}")]);

    let child = host_text(&host, "resources/views/posts/index.blade.php");
    let markers = plugin.line_markers(&doc("/project/resources/views/posts/index.blade.php", "blade", &child));
    let tooltips: Vec<_> = markers.iter().map(|m| (m.line, m.tooltip.as_str())).collect();
    assert_eq!(
        tooltips,
        [
            (0, "Rendered by PostController::index"),
            (1, "Parent section \"content\""),
            (4, "Stack \"scripts\"")
        ]
    );
    assert_eq!(markers[1].target.as_ref().unwrap().path, "/project/resources/views/layouts/app.blade.php");

    let layout = host_text(&host, "resources/views/layouts/app.blade.php");
    let markers = plugin.line_markers(&doc("/project/resources/views/layouts/app.blade.php", "blade", &layout));
    let tooltips: Vec<_> = markers.iter().map(|m| m.tooltip.as_str()).collect();
    assert_eq!(tooltips, ["Used by posts.index", "Section \"content\" is filled by another view", "Stack \"scripts\" is pushed to by another view"]);

    let nav = host_text(&host, "resources/views/partials/nav.blade.php");
    let markers = plugin.line_markers(&doc("/project/resources/views/partials/nav.blade.php", "blade", &nav));
    assert_eq!(markers[0].tooltip, "Used by posts.index");
}

#[test]
fn adds_missing_translation_keys() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let blade = doc("/project/resources/views/a.blade.php", "blade", "{{ __('messages.goodbye') }}{{ __('messages.nested.x') }}");

    let actions = plugin.code_actions(&blade, Range::new(Position::new(0, 16), Position::new(0, 16)));
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0].title, "Add translation key \"messages.goodbye\" to en/messages.php");

    let resolved = plugin.resolve_code_action(&blade, actions[0].clone()).unwrap();
    let (path, edits) = &resolved.edits[0];
    assert_eq!(path, "/project/lang/en/messages.php");
    assert_eq!(edits[0].text, "    'goodbye' => 'goodbye',\n");
    assert_eq!(edits[0].range.start, Position::new(7, 0));

    let nested = plugin.code_actions(&blade, Range::new(Position::new(0, 45), Position::new(0, 45)));
    assert!(nested.iter().all(|a| !a.title.starts_with("Add translation key")));
}

#[test]
fn shows_and_opens_names_in_the_tool_window() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let ctx = host.invocation();

    plugin.handle(VIEW, ViewEvent::Expand("group:route".into()), &ctx).unwrap();
    let View { body: Body::Tree(items), .. } = plugin.render(VIEW, None, &ctx).unwrap() else { panic!("not a tree") };
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).take(6).collect();
    assert_eq!(labels, ["Routes (4)", "GET|HEAD /", "GET|HEAD /posts", "GET|HEAD /posts/{post}", "GET|HEAD /admin/dashboard", "Views (7)"]);

    plugin.handle(VIEW, ViewEvent::Activate("route:0".into()), &ctx).unwrap();
    let routes = host_text(&host, "routes/web.php");
    let name_at = text::position(&routes, routes.find("'home'").unwrap() + 1);
    assert_eq!(host.opened_locations(), [("/project/routes/web.php".into(), name_at.line, name_at.character)]);
}

#[test]
fn runs_artisan_then_reindexes() {
    let host = MockHost::install().with_capabilities(&["project.read", "events", "process:php"]);
    let mut plugin = activated(&host);
    host.on_process("php", |args, _| {
        assert_eq!(args, ["artisan", "cache:clear", "--no-interaction"]);
        Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: "ok".into(), stderr: String::new() })
    });
    assert_eq!(plugin.run_command("cache-clear", &host.invocation(), None).unwrap(), None);
    let notes: Vec<_> = host.notifications().into_iter().map(|n| (n.title, n.body)).collect();
    assert_eq!(notes, [("Laravel".into(), "Ran artisan cache:clear and reindexed the project.".into())]);
}

#[test]
fn ignores_projects_without_laravel() {
    let host = MockHost::install();
    host.add_file("composer.json", r#"{"require": {"symfony/framework-bundle": "^7.0"}}"#);
    let mut plugin = Main::default();
    plugin.activate(&host.context("aritti.laravel")).unwrap();
    assert_eq!(plugin.run_command("reindex", &host.invocation(), None).unwrap(), None);
    assert_eq!(host.notifications()[0].body, "No Laravel project is open.");
}

fn activated_with(host: &MockHost, files: &[(&str, &str)]) -> Main {
    laravel_project(host);
    for (path, text) in files {
        host.add_file(path, text);
    }
    let mut plugin = Main::default();
    plugin.activate(&host.context("aritti.laravel")).unwrap();
    plugin
}

fn codes(plugin: &Main, language: &str, text: &str) -> Vec<String> {
    let index = plugin.index_for("/project/x").unwrap();
    problems(index, language, text, false).into_iter().map(|d| d.code.unwrap()).collect()
}

#[test]
fn unknown_blade_component_warns() {
    let host = MockHost::install();
    let plugin = activated(&host);

    assert_eq!(codes(&plugin, "blade", "<x-nope />\n<x-card>a</x-card>\n<x-forms.input-field />\n<x-panel />"), ["laravel.unknown-component"]);
    assert!(codes(&plugin, "blade", "{{-- <x-nope /> --}}\n<x-slot name=\"a\"></x-slot>\n<x-slot:title>t</x-slot:title>\n<x-dynamic-component :component=\"$c\" />\n<x-{{ $name }} />").is_empty());
}

#[test]
fn registered_blade_components_are_quiet() {
    let host = MockHost::install();
    let plugin = activated_with(
        &host,
        &[("app/Providers/AppServiceProvider.php", "<?php\nBlade::component('package-alert', Alert::class);\n"), ("resources/views/components/card/card.blade.php", "")],
    );

    assert!(codes(&plugin, "blade", "<x-package-alert /><x-card />").is_empty());

    let host = MockHost::install();
    let plugin = activated_with(&host, &[("app/Providers/AppServiceProvider.php", "<?php\nBlade::anonymousComponentPath(__DIR__.'/x');\n")]);
    assert!(codes(&plugin, "blade", "<x-nope />").is_empty());
}

#[test]
fn package_component_namespace_is_skipped() {
    let host = MockHost::install();
    let plugin = activated(&host);

    assert!(codes(&plugin, "blade", "<x-package::alert />").is_empty());
    assert!(codes(&plugin, "blade", "<livewire:package::alert />").is_empty());
}

#[test]
fn unknown_livewire_component_warns() {
    let host = MockHost::install();
    let plugin = activated(&host);

    assert_eq!(codes(&plugin, "blade", "<livewire:nope />"), ["laravel.unknown-livewire"]);
    assert_eq!(codes(&plugin, "blade", "@livewire('nope')"), ["laravel.unknown-livewire-directive"]);
    assert!(codes(&plugin, "blade", "<livewire:styles /><livewire:scripts /><livewire:is :component=\"$c\" />@livewire($name)").is_empty());
}

#[test]
fn known_livewire_component_is_quiet() {
    let host = MockHost::install();
    let plugin = activated_with(&host, &[("resources/views/livewire/volt-counter.blade.php", "")]);

    assert!(codes(&plugin, "blade", "<livewire:counter />\n@livewire('counter')\n<livewire:volt-counter />").is_empty());
}

#[test]
fn unknown_inertia_page_warns() {
    let host = MockHost::install();
    let plugin = activated_with(&host, &[("resources/js/Pages/Posts/Index.vue", ""), ("resources/js/pages/Home.tsx", "")]);

    let php = "<?php\nInertia::render('Missing/Page');\nInertia::render('Posts/Index');\ninertia('Home');\nRoute::inertia('/x', 'Nope');\nInertia::render('Posts/' . $x);\n";
    assert_eq!(codes(&plugin, "php", php), ["laravel.unknown-inertia-page", "laravel.unknown-inertia-page"]);

    let bare = MockHost::install();
    let plugin = activated(&bare);
    assert!(codes(&plugin, "php", "<?php\nInertia::render('Missing/Page');").is_empty());
}

#[test]
fn unknown_factory_for_model_warns() {
    let host = MockHost::install();
    let plugin = activated_with(
        &host,
        &[
            ("database/factories/ArticleFactory.php", "<?php\nclass ArticleFactory {}\n"),
            ("app/Models/Tag.php", "<?php\nclass Tag extends Model\n{\n    protected static function newFactory() {}\n}\n"),
        ],
    );

    let php = "<?php\nPost::factory()->create();\nArticle::factory();\nTag::factory();\nUnknown::factory();\nstatic::factory();\n";
    let found = problems(plugin.index_for("/project/x").unwrap(), "php", php, false);
    assert_eq!(found.iter().map(|d| d.code.clone().unwrap()).collect::<Vec<_>>(), ["laravel.unknown-factory"]);
    assert_eq!((found[0].range.start.line, found[0].range.start.character, found[0].range.end.character), (1, 0, 4));
}

#[test]
fn switched_off_checks_are_quiet() {
    let host = MockHost::install();
    let plugin = activated(&host);
    for key in ["reportUnknownComponents", "reportUnknownLivewire", "reportUnknownLivewireDirectives", "reportUnknownInertiaPages", "reportUnknownFactories"] {
        host.set_setting(key, "false");
    }

    assert!(codes(&plugin, "blade", "<x-nope /><livewire:nope />@livewire('nope')").is_empty());
    assert!(codes(&plugin, "php", "<?php\nPost::factory();").is_empty());
}

#[test]
fn vendor_component_packages_keep_components_quiet() {
    let host = MockHost::install();
    let plugin = activated_with(&host, &[("composer.json", r#"{"require": {"laravel/framework": "^11.0", "blade-ui-kit/blade-heroicons": "^2"}}"#)]);
    assert!(codes(&plugin, "blade", "<x-heroicon-o-user />").is_empty());

    let host = MockHost::install();
    let plugin = activated(&host);
    assert_eq!(codes(&plugin, "blade", "<x-heroicon-o-user />"), ["laravel.unknown-component"]);
}

#[test]
fn registered_livewire_components_keep_livewire_quiet() {
    let host = MockHost::install();
    let plugin = activated_with(&host, &[("app/Providers/AppServiceProvider.php", "<?php\nLivewire::component('shop-cart', Cart::class);\n")]);
    assert!(codes(&plugin, "blade", "<livewire:shop-cart />@livewire('shop-cart')").is_empty());
}

#[test]
fn blade_component_without_alias_is_derived_or_quiet() {
    let host = MockHost::install();
    let plugin = activated_with(&host, &[("app/Providers/AppServiceProvider.php", "<?php\nBlade::component(\\Pkg\\FancyAlert::class);\n")]);
    assert_eq!(codes(&plugin, "blade", "<x-fancy-alert /><x-nope />"), ["laravel.unknown-component"]);

    let host = MockHost::install();
    let plugin = activated_with(&host, &[("app/Providers/AppServiceProvider.php", "<?php\nBlade::component($class);\n")]);
    assert!(codes(&plugin, "blade", "<x-nope />").is_empty());
}

#[test]
fn partial_index_stays_quiet() {
    let host = MockHost::install();
    let mut plugin = activated_with(&host, &[("resources/js/Pages/Home.vue", ""), ("database/factories/ArticleFactory.php", "<?php\n")]);
    let root = plugin.indexes.keys().next().unwrap().clone();
    plugin.indexes.get_mut(&root).unwrap().partial = true;

    let blade = "<x-nope /><livewire:nope />@livewire('nope')";
    let php = "<?php\nInertia::render('Nope');\nPost::factory();\n";
    assert!(codes(&plugin, "blade", blade).is_empty());
    assert!(codes(&plugin, "php", php).is_empty());

    plugin.indexes.get_mut(&root).unwrap().partial = false;
    assert_eq!(codes(&plugin, "blade", blade).len(), 3);
    assert_eq!(codes(&plugin, "php", php).len(), 2);
}

fn rewrite_titles(plugin: &mut Main, path: &str, language: &str, text: &str, marker: &str) -> Vec<String> {
    let at = text.find(marker).unwrap() + 1;
    let position = text::position(text, at);
    plugin.code_actions(&doc(path, language, text), Range::new(position, position)).into_iter().map(|a| a.title).collect()
}

fn apply_rewrite(plugin: &mut Main, path: &str, language: &str, text: &str, marker: &str) -> String {
    let at = text.find(marker).unwrap() + 1;
    let position = text::position(text, at);
    let document = doc(path, language, text);
    let action = plugin.code_actions(&document, Range::new(position, position)).remove(0);
    let mut edits: Vec<_> = action.edits[0].1.iter().map(|e| (text::offset(text, e.range.start).unwrap(), text::offset(text, e.range.end).unwrap(), e.text.clone())).collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut result = text.to_owned();
    for (start, end, replacement) in edits {
        result.replace_range(start..end, &replacement);
    }
    result
}

#[test]
fn validation_string_to_array() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let path = "/project/app/Http/Controllers/PostController.php";
    let text = "<?php\n$request->validate(['email' => 'required|email|max:255', 'slug' => 'regex:/a|b/|required', 'one' => 'required', 'name' => [\"a\"]]);\n";

    let result = apply_rewrite(&mut plugin, path, "php", text, "required|email");
    assert!(result.contains("'email' => ['required', 'email', 'max:255'],"));
    assert!(result.contains("'regex:/a|b/|required'"));

    assert!(rewrite_titles(&mut plugin, path, "php", text, "regex:").is_empty());
    assert!(rewrite_titles(&mut plugin, path, "php", "<?php\n$x = ['a' => 'b|c'];\n", "b|c").is_empty());
}

#[test]
fn validation_one_rule_is_left_alone() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\n$request->validate(['one' => 'required']);\n";
    assert!(rewrite_titles(&mut plugin, "/project/app/A.php", "php", text, "required").is_empty());
}

#[test]
fn controller_string_to_class_array() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let path = "/project/routes/web.php";

    let imported = "<?php\nuse App\\Http\\Controllers\\UserController;\nRoute::get('/u', 'UserController@index');\n";
    let result = apply_rewrite(&mut plugin, path, "php", imported, "UserController@");
    assert_eq!(result, "<?php\nuse App\\Http\\Controllers\\UserController;\nRoute::get('/u', [UserController::class, 'index']);\n");

    let same_namespace = "<?php\nnamespace App\\Http\\Controllers;\nRoute::get('/u', 'UserController@index');\n";
    assert!(apply_rewrite(&mut plugin, path, "php", same_namespace, "UserController@").contains("[UserController::class, 'index']"));

    let not_imported = "<?php\nRoute::get('/u', 'UserController@index');\n";
    assert!(rewrite_titles(&mut plugin, path, "php", not_imported, "UserController@").is_empty());

    let other_import = "<?php\nuse App\\Other\\PostController;\nRoute::get('/u', 'UserController@index');\n";
    assert!(rewrite_titles(&mut plugin, path, "php", other_import, "UserController@").is_empty());
}

#[test]
fn blade_braces_escape_toggle() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let path = "/project/resources/views/a.blade.php";

    let escaped = "<p>{{ $x }}</p>\n";
    assert_eq!(apply_rewrite(&mut plugin, path, "blade", escaped, "$x"), "<p>{!! $x !!}</p>\n");

    let raw = "<p>{!! $x !!}</p>\n";
    assert_eq!(apply_rewrite(&mut plugin, path, "blade", raw, "$x"), "<p>{{ $x }}</p>\n");

    assert!(rewrite_titles(&mut plugin, path, "blade", "{{-- $x --}}", "$x").is_empty());
    assert!(rewrite_titles(&mut plugin, path, "blade", "@{{ $x }}", "$x").is_empty());
    assert!(rewrite_titles(&mut plugin, path, "blade", "<p>text</p>", "text").is_empty());
    assert!(rewrite_titles(&mut plugin, "/project/app/a.php", "php", "<?php echo '{{ $x }}';", "$x").is_empty());
}

#[test]
fn validation_string_inside_existing_array_is_left_alone() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\n$request->validate(['a' => ['required', 'string|max:5']]);\n";
    assert!(rewrite_titles(&mut plugin, "/project/app/A.php", "php", text, "string|max").is_empty());
}

#[test]
fn validation_custom_messages_are_left_alone() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let path = "/project/app/A.php";
    let text = "<?php\n$request->validate(['email' => 'required|email'], ['email.required' => 'Required|please']);\nValidator::make($d, ['n' => 'a|b'], ['n.a' => 'x|y']);\n";
    assert!(rewrite_titles(&mut plugin, path, "php", text, "Required|please").is_empty());
    assert!(rewrite_titles(&mut plugin, path, "php", text, "x|y").is_empty());
    assert_eq!(rewrite_titles(&mut plugin, path, "php", text, "a|b").len(), 1);
}

#[test]
fn rewrites_survive_multibyte_text_and_crlf() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let path = "/project/app/A.php";
    let text = "<?php\r\n// é日本\r\n$request->validate(['é' => 'required|email']);\r\n";
    let result = apply_rewrite(&mut plugin, path, "php", text, "required|email");
    assert_eq!(result, "<?php\r\n// é日本\r\n$request->validate(['é' => ['required', 'email']]);\r\n");

    let blade = "<p>é</p>\r\n<p>{{ $x }}</p>\r\n";
    let result = apply_rewrite(&mut plugin, "/project/resources/views/a.blade.php", "blade", blade, "$x");
    assert_eq!(result, "<p>é</p>\r\n<p>{!! $x !!}</p>\r\n");
}

#[test]
fn controller_this_validate_rules_are_offered() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\n$this->validate($request, ['email' => 'required|email'], ['email.required' => 'a|b']);\n";
    let result = apply_rewrite(&mut plugin, "/project/app/A.php", "php", text, "required|email");
    assert!(result.contains("['email' => ['required', 'email']]"));
}

#[test]
fn controller_this_validate_messages_are_left_alone() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\n$this->validate($request, ['email' => 'required|email'], ['email.required' => 'a|b']);\n";
    assert!(rewrite_titles(&mut plugin, "/project/app/A.php", "php", text, "a|b").is_empty());
}

#[test]
fn bag_and_form_request_rules_are_offered() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let path = "/project/app/A.php";
    let bag = "<?php\n$request->validateWithBag('post', ['t' => 'required|max:5']);\n$this->validateWithBag('post', $request, ['u' => 'required|max:6']);\n";
    assert_eq!(rewrite_titles(&mut plugin, path, "php", bag, "required|max:5").len(), 1);
    assert_eq!(rewrite_titles(&mut plugin, path, "php", bag, "required|max:6").len(), 1);

    let form = "<?php\nclass R {\n    public function rules(): array\n    {\n        return ['n' => 'required|string'];\n    }\n}\n";
    assert_eq!(rewrite_titles(&mut plugin, path, "php", form, "required|string").len(), 1);
}

#[test]
fn chained_and_livewire_validate_rules_are_offered() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let path = "/project/app/A.php";
    let text = "<?php\n$this->request->validate(['a' => 'required|email']);\n$this->request->validateWithBag('b', ['c' => 'required|max:9']);\n$this->validate(['x' => 'required|url']);\n$this->validate($request, ['y' => 'required|ip'], ['y.ip' => 'p|q']);\n$this->validateWithBag('b', ['z' => 'required|uuid'], ['z.uuid' => 'm|n']);\n";
    for marker in ["required|email", "required|max:9", "required|url", "required|ip", "required|uuid"] {
        assert_eq!(rewrite_titles(&mut plugin, path, "php", text, marker).len(), 1, "{marker}");
    }
    for marker in ["p|q", "m|n"] {
        assert!(rewrite_titles(&mut plugin, path, "php", text, marker).is_empty(), "{marker}");
    }
}

#[test]
fn every_template_is_registered_and_parses() {
    let manifest = include_str!("../plugin.toml");
    let paths: Vec<&str> = manifest
        .lines()
        .filter_map(|line| line.strip_prefix("path = \""))
        .filter(|rest| rest.starts_with("templates/"))
        .filter_map(|rest| rest.strip_suffix('"'))
        .collect();
    assert_eq!(paths.len(), 16, "{paths:?}");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in paths {
        let body = std::fs::read_to_string(root.join(path)).unwrap_or_else(|_| panic!("missing {path}"));
        assert!(body.starts_with("<?php") || body.starts_with("<?php\n"), "{path}");
        assert!(!body.contains("{{"), "{path}");
        let mut rest = body.as_str();
        while let Some(start) = rest.find('$') {
            let after = &rest[start + 1..];
            let is_var = after.find('$').map(|end| &after[..end]).filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_uppercase() || c == '_'));
            if let Some(var) = is_var {
                assert!(["NAME", "END"].contains(&var), "{path}: ${var}$");
                rest = &after[var.len() + 1..];
            } else {
                rest = after;
            }
        }
    }
}

fn manifest_values(header: &str, key: &str) -> Vec<String> {
    let manifest = include_str!("../plugin.toml");
    let prefix = format!("{key} = ");
    let mut inside = false;
    let mut values = Vec::new();
    for line in manifest.lines() {
        if line.starts_with('[') {
            inside = line == header;
        }
        if inside && let Some(value) = line.strip_prefix(&prefix) {
            values.push(value.to_owned());
        }
    }
    values
}

#[test]
fn commands_declared_in_manifest_have_handlers() {
    let ids = manifest_values("[[contributes.commands]]", "id");
    assert!(ids.len() >= 10, "{ids:?}");
    for quoted in ids {
        let id = quoted.trim_matches('"');
        let host = MockHost::install();
        let result = Main::default().run_command(id, &host.invocation(), None);
        assert!(!matches!(result, Err(Error::NotFound(_))), "no handler for {id}");
    }
}

#[test]
fn run_detectors_run_artisan_through_php() {
    let header = "[[contributes.runDetectors]]";
    assert_eq!(manifest_values(header, "files"), ["[\"artisan\"]"; 4]);
    assert_eq!(manifest_values(header, "command"), ["\"php\""; 4]);
    assert_eq!(
        manifest_values(header, "args"),
        [
            "[\"artisan\", \"serve\"]",
            "[\"artisan\", \"test\"]",
            "[\"artisan\", \"queue:work\"]",
            "[\"artisan\", \"schedule:work\"]",
        ]
    );
}

#[test]
fn migrate_fresh_runs_only_when_confirmed() {
    let host = MockHost::install().with_capabilities(&["project.read", "events", "process:php"]);
    let mut plugin = activated(&host);
    host.on_process("php", |args, _| {
        assert_eq!(args, ["artisan", "migrate:fresh", "--no-interaction"]);
        Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: "ok".into(), stderr: String::new() })
    });
    plugin.run_command("migrate-fresh", &host.invocation(), None).unwrap();
    assert!(host.notifications()[0].body.contains("again within 30 seconds"));
    assert_eq!(host.notifications().len(), 1);
    plugin.run_command("migrate-fresh", &host.invocation(), None).unwrap();
    assert_eq!(host.notifications().len(), 2);
    plugin.run_command("migrate-fresh", &host.invocation(), None).unwrap();
    assert_eq!(host.notifications().len(), 3);
    assert!(host.notifications()[2].body.contains("again within"));
}

#[test]
fn migrate_fresh_confirmation_is_per_project() {
    let host = MockHost::install().with_capabilities(&["project.read", "events", "process:php"]);
    let mut plugin = activated(&host);
    let ran = std::rc::Rc::new(std::cell::Cell::new(0));
    let counter = ran.clone();
    host.on_process("php", move |_, _| {
        counter.set(counter.get() + 1);
        Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: "ok".into(), stderr: String::new() })
    });

    host.set_project_root(Some("/project-a"));
    plugin.run_command("migrate-fresh", &host.invocation(), None).unwrap();
    host.set_project_root(Some("/project-b"));
    plugin.run_command("migrate-fresh", &host.invocation(), None).unwrap();

    assert_eq!(ran.get(), 0);
    assert_eq!(host.notifications().len(), 2);

    plugin.run_command("migrate-fresh", &host.invocation(), None).unwrap();
    assert_eq!(ran.get(), 1);
}

fn complete_directives(plugin: &mut Main, marked: &str) -> Vec<String> {
    let (text, at) = cursor(marked);
    labels(plugin.completion(&doc("/project/resources/views/posts/show.blade.php", "blade", &text), at, None))
}

#[test]
fn completes_core_and_livewire_directives_after_at() {
    let host = MockHost::install();
    let mut plugin = activated(&host);

    let all = complete_directives(&mut plugin, "<div>\n@|\n</div>");
    for name in ["if", "foreach", "forelse", "section", "livewire", "vite", "endif"] {
        assert!(all.contains(&name.to_owned()), "missing {name}");
    }
    assert!(complete_directives(&mut plugin, "@fo|").contains(&"foreach".to_owned()));
    assert!(complete_directives(&mut plugin, "@forel|rest").contains(&"forelse".to_owned()));
}

#[test]
fn completes_custom_directive_from_project() {
    let host = MockHost::install();
    let mut plugin = activated_with(
        &host,
        &[
            ("app/Providers/AppServiceProvider.php", "<?php\nBlade::directive('datetime', fn ($e) => $e);\n"),
            ("app/Support/Directives.php", "<?php\nBlade::directive(\"money\", fn ($e) => $e);\n// Blade::directive('commented', fn () => 1);\n"),
        ],
    );

    let names = complete_directives(&mut plugin, "@dat|");
    assert_eq!(&names[..2], ["datetime", "money"]);
    assert!(names.contains(&"if".to_owned()) && !names.contains(&"commented".to_owned()));
}

#[test]
fn escaped_at_is_not_completed() {
    let host = MockHost::install();
    let mut plugin = activated(&host);

    assert!(complete_directives(&mut plugin, "@@i|").is_empty());
    assert!(complete_directives(&mut plugin, "mail user@exa|").is_empty());
    assert!(complete_directives(&mut plugin, "user@|").is_empty());
    assert!(complete_directives(&mut plugin, "{{ $a @fo| }}").is_empty());
}

#[test]
fn directive_list_matches_the_blade_plugin() {
    let source = include_str!("../../blade/src/lib.rs");
    let region = &source[source.find("const BLOCKS").unwrap()..source.find("fn is_directive").unwrap()];
    let in_blade: std::collections::BTreeSet<&str> = region.split('"').skip(1).step_by(2).collect();
    let here: std::collections::BTreeSet<&str> = crate::directives::CORE.iter().copied().collect();
    assert_eq!(here, in_blade);
}
