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

fn project_files(files: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |path| files.iter().find(|(name, _)| *name == path).map(|(_, text)| (*text).to_owned())
}

const UNGUARDED_POST: (&str, &str) = ("app/Models/Post.php", "<?php\nclass Post extends Model\n{\n    protected $guarded = [];\n}\n");
const FILLABLE_POST: (&str, &str) = ("app/Models/Post.php", "<?php\nclass Post extends Model\n{\n    protected $fillable = ['title', 'body'];\n}\n");
const PRUNABLE_POST: (&str, &str) = ("app/Models/Post.php", "<?php\nuse Illuminate\\Database\\Eloquent\\Prunable;\nclass Post extends Model\n{\n    use HasFactory, Prunable;\n}\n");

fn eloquent_snippets(text: &str, files: &'static [(&'static str, &'static str)]) -> Vec<(&'static str, String)> {
    eloquent_checks::findings(text, &project_files(files)).into_iter().map(|f| (f.code, text[f.start..f.end].to_owned())).collect()
}

#[test]
fn guarded_attribute_assigned_through_fill_warns() {
    static FILES: [(&str, &str); 1] = [UNGUARDED_POST];
    let found = eloquent_snippets("<?php\nnamespace App\\Http;\nuse App\\Models\\Post;\n(new Post)->fill($request->all());\nPost::create($request->all());\nPost::create($request->validated());\n", &FILES);
    assert_eq!(found, [("guarded-attribute-assignment", "$request->all()".into()), ("guarded-attribute-assignment", "$request->all()".into())]);

    static FILLABLE: [(&str, &str); 1] = [FILLABLE_POST];
    let found = eloquent_snippets("<?php\nnamespace App\\Http;\nuse App\\Models\\Post;\nPost::create(['title' => 'a', 'role' => 'admin', 'body' => 'b']);\nPost::create($request->all());\n", &FILLABLE);
    assert_eq!(found, [("guarded-attribute-assignment", "role".into())]);
}

#[test]
fn guarded_attribute_assignment_is_quiet_for_library_or_dynamic_name() {
    static FILES: [(&str, &str); 1] = [UNGUARDED_POST];
    let text = "<?php\nnamespace App\\Http;\nuse App\\Models\\Post;\nCollection::create($request->all());\n$model::create($request->all());\nstatic::create($request->all());\nPost::create($request->only('a'));\n$post->fill($request->all());\n";
    assert!(eloquent_snippets(text, &FILES).is_empty());

    static FILLABLE: [(&str, &str); 1] = [FILLABLE_POST];
    assert!(eloquent_snippets("<?php\nnamespace App\\Http;\nuse App\\Models\\Post;\nPost::create(['title' => 1, $key => 2, ...$rest]);\nPost::create(['title' => 1, 'body' => 2]);\n", &FILLABLE).is_empty());

    static CUSTOM: [(&str, &str); 1] = [("app/Models/Post.php", "<?php\nclass Post extends Model\n{\n    protected $fillable = ['a'];\n    public function getFillable() {}\n}\n")];
    assert!(eloquent_snippets("<?php\nnamespace App\\Http;\nuse App\\Models\\Post;\nPost::create(['zzz' => 1]);\n", &CUSTOM).is_empty());
}

#[test]
fn prunable_model_without_schedule_warns() {
    static NO_SCHEDULE: [(&str, &str); 1] = [("routes/console.php", "<?php\nSchedule::command('inspire')->hourly();\n")];
    let found = eloquent_snippets(PRUNABLE_POST.1, &NO_SCHEDULE);
    assert_eq!(found, [("unscheduled-prunable", "Prunable".into())]);

    let mass = PRUNABLE_POST.1.replace("Prunable;\n}", "MassPrunable;\n}").replace("HasFactory, Prunable", "HasFactory, MassPrunable");
    assert_eq!(eloquent_snippets(&mass, &NO_SCHEDULE).len(), 1);
}

#[test]
fn unscheduled_prunable_is_quiet_for_library_or_dynamic_name() {
    static SCHEDULED: [(&str, &str); 2] = [("routes/console.php", "<?php\nSchedule::command('inspire');\n"), ("bootstrap/app.php", "<?php\n->withSchedule(fn ($s) => $s->command('model:prune')->daily())")];
    assert!(eloquent_snippets(PRUNABLE_POST.1, &SCHEDULED).is_empty());

    static NOTHING_READABLE: [(&str, &str); 0] = [];
    assert!(eloquent_snippets(PRUNABLE_POST.1, &NOTHING_READABLE).is_empty());

    static NO_SCHEDULE: [(&str, &str); 1] = [("routes/console.php", "<?php\n")];
    let text = "<?php\nuse Illuminate\\Database\\Eloquent\\Prunable;\nclass Post extends Model\n{\n    use HasFactory;\n    // use Prunable;\n}\n";
    assert!(eloquent_snippets(text, &NO_SCHEDULE).is_empty());
}

#[test]
fn order_by_created_at_desc_becomes_latest() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let path = "/project/app/Http/Controllers/PostController.php";

    let text = "<?php\n$a = Post::orderBy('created_at', 'desc')->get();\n$b = Post::query()->orderBy(\"created_at\")->get();\n$c = Post::orderBy('created_at', 'ASC')->get();\n";
    assert_eq!(apply_rewrite(&mut plugin, path, "php", text, "orderBy('created_at', 'desc')"), text.replace("orderBy('created_at', 'desc')", "latest()"));
    assert_eq!(apply_rewrite(&mut plugin, path, "php", text, "orderBy(\"created_at\")"), text.replace("orderBy(\"created_at\")", "oldest()"));
    assert_eq!(apply_rewrite(&mut plugin, path, "php", text, "orderBy('created_at', 'ASC')"), text.replace("orderBy('created_at', 'ASC')", "oldest()"));
    assert_eq!(rewrite_titles(&mut plugin, path, "php", text, "orderBy('created_at', 'desc')").len(), 1);
}

#[test]
fn simplify_order_by_is_quiet_for_library_or_dynamic_name() {
    static NOTHING: [(&str, &str); 0] = [];
    let text = "<?php\n$a = Post::orderBy('title', 'desc');\n$b = Post::orderBy($column, 'desc');\n$c = Post::orderBy('created_at', $direction);\n$d = Post::orderBy('created_at', 'desc', 1);\n$e = Post::orderByDesc('created_at');\n// Post::orderBy('created_at');\n$f = orderBy('created_at');\n";
    assert!(eloquent_snippets(text, &NOTHING).is_empty());
}

#[test]
fn eloquent_checks_publish_and_clear_when_the_file_is_fixed() {
    let host = MockHost::install();
    let mut plugin = activated_with(&host, &[("app/Models/Post.php", UNGUARDED_POST.1), ("app/Http/Controllers/C.php", "<?php\nnamespace App\\Http;\nuse App\\Models\\Post;\nPost::create($request->all());\n")]);
    let path = "/project/app/Http/Controllers/C.php";

    plugin.handle_events(&[Event::FileOpened(path.into())], &host.invocation());
    let found = &host.diagnostics()[path];
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].code.as_deref(), Some("guarded-attribute-assignment"));

    host.add_file("app/Http/Controllers/C.php", "<?php\nnamespace App\\Http;\nuse App\\Models\\Post;\nPost::create($request->validated());\n");
    plugin.handle_events(&[Event::FileOpened(path.into())], &host.invocation());
    assert!(host.diagnostics().get(path).is_none_or(Vec::is_empty));

    host.add_file("app/Http/Controllers/C.php", "<?php\nnamespace App\\Http;\nuse App\\Models\\Post;\nPost::create($request->all());\n");
    host.set_setting("checkEloquent", "false");
    plugin.handle_events(&[Event::FileOpened(path.into())], &host.invocation());
    assert!(host.diagnostics().get(path).is_none_or(Vec::is_empty));
}

fn guarded_mass_assignment_in(relative: &str) -> Vec<Diagnostic> {
    let host = MockHost::install();
    let body = "<?php\nnamespace Database;\nuse App\\Models\\Post;\nPost::create($request->all());\n";
    let mut plugin = activated_with(&host, &[("app/Models/Post.php", UNGUARDED_POST.1), (relative, body)]);
    let path = format!("/project/{relative}");

    plugin.handle_events(&[Event::FileOpened(path.clone())], &host.invocation());
    host.diagnostics().get(&path).cloned().unwrap_or_default()
}

#[test]
fn seeder_mass_assignment_is_quiet() {
    assert!(guarded_mass_assignment_in("database/seeders/PostSeeder.php").is_empty());
}

#[test]
fn factory_mass_assignment_is_quiet() {
    assert!(guarded_mass_assignment_in("database/factories/PostFactory.php").is_empty());
}

#[test]
fn app_file_mass_assignment_still_warns() {
    assert_eq!(guarded_mass_assignment_in("app/Http/Controllers/PostController.php").len(), 1);
}

#[test]
fn eloquent_findings_use_utf16_columns_and_crlf() {
    let host = MockHost::install();
    let body = "<?php\r\n// é日本😀\r\n$x = 'é日'; Post::orderBy('created_at', 'desc');\r\n";
    let mut plugin = activated_with(&host, &[("app/Http/Controllers/C.php", body)]);
    let path = "/project/app/Http/Controllers/C.php";

    plugin.handle_events(&[Event::FileOpened(path.into())], &host.invocation());
    let range = host.diagnostics()[path][0].range;
    assert_eq!((range.start.line, range.start.character, range.end.character), (2, 17, 46));
    assert_eq!(host.diagnostics()[path][0].severity, Severity::Hint);

    let result = apply_rewrite(&mut plugin, path, "php", body, "orderBy('created");
    assert_eq!(result, "<?php\r\n// é日本😀\r\n$x = 'é日'; Post::latest();\r\n");
}

#[test]
fn rule_array_becomes_string() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let path = "/project/app/Http/Controllers/PostController.php";
    let text = "<?php\n$request->validate([\n    'email' => ['required', 'email', 'max:255'],\n    'a' => [\"x\", \"y\"],\n]);\n";

    let result = apply_rewrite(&mut plugin, path, "php", text, "'required'");
    assert!(result.contains("'email' => 'required|email|max:255',"));
    let result = apply_rewrite(&mut plugin, path, "php", text, "\"x\"");
    assert!(result.contains("'a' => \"x|y\","));
}

#[test]
fn rule_array_with_objects_or_regex_is_left_alone() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let path = "/project/app/A.php";
    let titles = |plugin: &mut Main, text: &str, marker: &str| rewrite_titles(plugin, path, "php", text, marker);

    assert!(titles(&mut plugin, "<?php\n$request->validate(['a' => ['required', new Uppercase]]);\n", "'required'").is_empty());
    assert!(titles(&mut plugin, "<?php\n$request->validate(['a' => ['required', fn ($v) => true]]);\n", "'required'").is_empty());
    assert!(titles(&mut plugin, "<?php\n$request->validate(['a' => ['required', 'regex:/a|b/']]);\n", "'required'").is_empty());
    assert!(titles(&mut plugin, "<?php\n$request->validate(['a' => ['required', 'not_regex:/a{1,2}/']]);\n", "'required'").is_empty());
    assert!(titles(&mut plugin, "<?php\n$request->validate(['a' => ['required', Rule::in(['x'])]]);\n", "'required'").is_empty());
    assert!(titles(&mut plugin, "<?php\n$x = ['a' => ['required', 'email']];\n", "'required'").is_empty());
    assert!(titles(&mut plugin, "<?php\n$request->validate(['a' => 'required'], ['a.required' => ['x', 'y']]);\n", "'x'").is_empty());
}

#[test]
fn intentions_are_offered_only_at_the_caret() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let path = "/project/app/Http/Controllers/PostController.php";
    let text = "<?php\n$request->validate(['email' => ['required', 'email']]);\n$y = Post::orderBy('created_at');\n";

    assert_eq!(rewrite_titles(&mut plugin, path, "php", text, "required").len(), 1);
    assert_eq!(rewrite_titles(&mut plugin, path, "php", text, "orderBy").len(), 1);
    assert!(rewrite_titles(&mut plugin, path, "php", text, "validate(").is_empty());
    assert!(rewrite_titles(&mut plugin, path, "php", text, "$y").is_empty());
}

#[test]
fn vendor_class_sharing_a_project_model_name_is_quiet() {
    static FILES: [(&str, &str); 1] = [UNGUARDED_POST];
    for text in [
        "<?php\nVendor\\Pkg\\Post::create($request->all());\n",
        "<?php\n\\Vendor\\Post::create($request->all());\n",
        "<?php\nnamespace App\\Http;\nuse Vendor\\Post;\nPost::create($request->all());\n",
        "<?php\nnamespace App\\Http;\nuse Vendor\\Thing as Post;\nPost::create($request->all());\n",
        "<?php\nnamespace Vendor\\Pkg;\nPost::create($request->all());\n",
    ] {
        assert!(eloquent_snippets(text, &FILES).is_empty(), "{text}");
    }
}

#[test]
fn imported_project_model_still_warns() {
    static FILES: [(&str, &str); 1] = [UNGUARDED_POST];
    for text in [
        "<?php\nnamespace App\\Http;\nuse App\\Models\\Post;\nPost::create($request->all());\n",
        "<?php\n\\App\\Models\\Post::create($request->all());\n",
        "<?php\nnamespace App\\Http;\nuse App\\Models\\Post as Article;\nArticle::create($request->all());\n",
    ] {
        assert_eq!(eloquent_snippets(text, &FILES).len(), 1, "{text}");
    }
}

#[test]
fn app_internal_non_model_class_sharing_a_model_name_is_quiet() {
    static FILES: [(&str, &str); 2] = [UNGUARDED_POST, ("app/Services/Post.php", "<?php\nnamespace App\\Services;\nclass Post\n{\n    public static function create($data) {}\n}\n")];
    let text = "<?php\nnamespace App\\Http;\nuse App\\Services\\Post;\nPost::create($request->all());\n";
    assert!(eloquent_snippets(text, &FILES).is_empty());
}

#[test]
fn unimported_name_in_controller_namespace_is_quiet() {
    static FILES: [(&str, &str); 1] = [UNGUARDED_POST];
    let text = "<?php\nnamespace App\\Http\\Controllers;\nPost::create($request->all());\n";
    assert!(eloquent_snippets(text, &FILES).is_empty());
}

#[test]
fn namespaceless_file_with_unimported_name_is_quiet() {
    static FILES: [(&str, &str); 1] = [UNGUARDED_POST];
    assert!(eloquent_snippets("<?php\nPost::create($request->all());\n", &FILES).is_empty());
}

#[test]
fn group_import_of_vendor_class_is_quiet() {
    static FILES: [(&str, &str); 1] = [UNGUARDED_POST];
    let text = "<?php\nnamespace App\\Http;\nuse Vendor\\{Post, Comment};\nuse function Foo\\bar;\nPost::create($request->all());\n";
    assert!(eloquent_snippets(text, &FILES).is_empty());
}

#[test]
fn group_import_of_project_model_still_warns() {
    static FILES: [(&str, &str); 1] = [UNGUARDED_POST];
    let text = "<?php\nnamespace App\\Http;\nuse App\\Models\\{\n    Post,\n    Comment,\n};\nPost::create($request->all());\n";
    assert_eq!(eloquent_snippets(text, &FILES).len(), 1);
}

#[test]
fn group_import_alias_resolves() {
    static FILES: [(&str, &str); 1] = [UNGUARDED_POST];
    let text = "<?php\nnamespace App\\Http;\nuse App\\Models\\{Post as Article};\nArticle::create($request->all());\n";
    assert_eq!(eloquent_snippets(text, &FILES).len(), 1);

    let nested = "<?php\nnamespace App\\Http;\nuse App\\{Models\\Post};\nPost::create($request->all());\n";
    assert_eq!(eloquent_snippets(nested, &FILES).len(), 1);
}

fn js_complete(plugin: &mut Main, path: &str, language: &str, marked: &str) -> Vec<String> {
    let (text, at) = cursor(marked);
    labels(plugin.completion(&doc(path, language, &text), at, None))
}

const ROUTES: [&str; 4] = ["admin.dashboard", "home", "posts.index", "posts.show"];

#[test]
fn ziggy_route_completes_route_names() {
    let host = MockHost::install();
    let mut plugin = activated(&host);

    assert_eq!(js_complete(&mut plugin, "/project/resources/js/app.js", "javascript", "route('|')"), ROUTES);
    assert_eq!(js_complete(&mut plugin, "/project/resources/js/app.ts", "typescript", "router.visit(route(\"po|\"), {})"), ROUTES);
    assert_eq!(js_complete(&mut plugin, "/project/resources/js/app.tsx", "typescriptreact", "route(`|`)"), ROUTES);

    let (text, at) = cursor("route('|home')");
    let items = plugin.completion(&doc("/project/resources/js/app.js", "javascript", &text), at, None);
    let home = items.iter().find(|item| item.label == "home").unwrap();
    assert_eq!(home.detail.as_deref(), Some("GET|HEAD / → Closure"));
}

#[test]
fn ziggy_current_completes_route_names() {
    let host = MockHost::install();
    let mut plugin = activated(&host);

    assert_eq!(js_complete(&mut plugin, "/project/resources/js/a.js", "javascript", "route().current('|')"), ROUTES);
    assert_eq!(js_complete(&mut plugin, "/project/resources/js/a.js", "javascript", "route( ).current( 'po|' )"), ROUTES);
}

#[test]
fn vue_template_href_route_completes() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let vue = "<template>\n  <Link :href=\"route('|')\">Home</Link>\n</template>\n";

    assert_eq!(js_complete(&mut plugin, "/project/resources/js/Pages/Home.vue", "vue", vue), ROUTES);
}

#[test]
fn ziggy_route_definition_opens_the_route() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let routes = host_text(&host, "routes/web.php");
    let name_at = text::position(&routes, routes.find("'home'").unwrap() + 1);

    let script = doc("/project/resources/js/a.js", "javascript", "route('home');\nroute().current('posts.show');\n");
    let found = plugin.definition(&script, Position::new(0, 9));
    assert_eq!(found[0].path, "/project/routes/web.php");
    assert_eq!(found[0].range, Range::new(name_at, Position::new(name_at.line, name_at.character + 4)));
    assert_eq!(plugin.definition(&script, Position::new(1, 20)).len(), 1);
    assert!(plugin.definition(&doc("/project/resources/js/a.js", "javascript", "route('nope');"), Position::new(0, 9)).is_empty());
}

#[test]
fn js_provider_ignores_non_inertia_calls() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let path = "/project/resources/js/a.js";

    for marked in [
        "myroute('|')",
        "$route('|')",
        "app.route('|')",
        "this.route('|')",
        "router.get('|x')",
        "router.visit('|')",
        "// route('|')",
        "/* route('|') */",
        "const a = 1; // route('|')",
        "route(name, '|')",
        "route(foo('|'))",
        "other.current('|')",
        "route('home')|",
    ] {
        assert!(js_complete(&mut plugin, path, "javascript", marked).is_empty(), "{marked}");
    }
}

#[test]
fn completion_does_not_scan_project() {
    let host = MockHost::install();
    let mut plugin = Main::default();
    plugin.activate(&host.context("aritti.laravel")).unwrap();

    assert!(js_complete(&mut plugin, "/project/resources/js/a.js", "javascript", "route('|')").is_empty());
    assert!(plugin.definition(&doc("/project/resources/js/a.js", "javascript", "route('home')"), Position::new(0, 8)).is_empty());
}

#[test]
fn js_route_replace_range_is_utf16_and_covers_the_whole_name() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let (text, at) = cursor("const é = '😀'; route('ho|me.x')");
    let items = plugin.completion(&doc("/project/resources/js/a.js", "javascript", &text), at, None);

    let start = text.find("home").unwrap();
    let expected = Range::new(text::position(&text, start), text::position(&text, start + 6));
    assert_eq!(items[0].replace, Some(expected));
    assert_eq!(expected.start.character, 23);
}

#[test]
fn inertia_render_completes_page_names_from_resources_js_pages() {
    let host = MockHost::install();
    laravel_project(&host);
    host.add_file("resources/js/Pages/Posts/Index.vue", "");
    host.add_file("resources/js/pages/Home.tsx", "");
    let mut plugin = Main::default();
    plugin.activate(&host.context("aritti.laravel")).unwrap();

    let names = js_complete(&mut plugin, "/project/app/Http/Controllers/X.php", "php", "<?php Inertia::render('|');");
    assert_eq!(names, ["Home", "Posts/Index"]);
}

#[test]
fn inertia_page_extensions_vue_jsx_tsx_svelte() {
    let host = MockHost::install();
    laravel_project(&host);
    for file in ["A.vue", "B.jsx", "C.tsx", "D.svelte", "E.js"] {
        host.add_file(&format!("resources/js/Pages/{file}"), "");
    }
    let mut plugin = Main::default();
    plugin.activate(&host.context("aritti.laravel")).unwrap();

    let names = js_complete(&mut plugin, "/project/app/Http/Controllers/X.php", "php", "<?php inertia('|');");
    assert_eq!(names, ["A", "B", "C", "D"]);
}

fn route_names(marked: &str) -> Vec<String> {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    js_complete(&mut plugin, "/project/resources/js/a.js", "javascript", marked)
}

#[test]
fn glob_line_above_a_route_call_does_not_disable_completion() {
    let marked = "const pages = import.meta.glob('./Pages/**/*.vue');\nroute('|')";
    assert_eq!(route_names(marked), ROUTES);
}

#[test]
fn url_with_double_slash_in_a_string_does_not_make_a_comment() {
    assert_eq!(route_names("const u = 'https://example.test'; route('|')"), ROUTES);
}

#[test]
fn real_block_comment_still_silences() {
    assert!(route_names("/* note\n route('|') */").is_empty());
}

#[test]
fn real_line_comment_still_silences() {
    assert!(route_names("// route('|')").is_empty());
}

#[test]
fn template_literal_interpolation_is_quiet() {
    assert!(route_names("route(`a${x}|`)").is_empty());
}

#[test]
fn stray_backtick_does_not_poison_later_lines() {
    assert_eq!(route_names("const r = /`/;\nroute('|')"), ROUTES);
    assert_eq!(route_names("const r = /`/;\nroute(`|`)"), ROUTES);
}

#[test]
fn backtick_glob_does_not_open_a_block_comment_after_a_stray_backtick() {
    assert_eq!(route_names("const r = /`/;\nconst g = import.meta.glob(`./Pages/**/*.vue`);\nroute('|')"), ROUTES);
}

#[test]
fn slash_star_inside_a_multiline_template_literal_silences_later_lines() {
    let marked = "const t = `\n  ./Pages/*.vue\n`;\nroute('|')";
    assert!(route_names(marked).is_empty(), "accepted silence (safe direction), not a requirement");
}

#[test]
fn multiline_template_plain_text_after_interpolation_is_quiet() {
    assert!(route_names("const t = `a ${x}\nplain |text`;\nroute('home')").is_empty());
}

fn env_snippets(path: &str, language: &str, text: &str) -> Vec<(&'static str, String)> {
    function_checks::env_outside_config(path, language, text).into_iter().map(|f| (f.code, text[f.start..f.end].to_owned())).collect()
}

fn env_problems_of(host: &MockHost, relative: &str, body: &str) -> Vec<Diagnostic> {
    host.add_file(relative, body);
    let mut plugin = activated(host);
    let path = format!("/project/{relative}");

    plugin.handle_events(&[Event::FileOpened(path.clone())], &host.invocation());
    host.diagnostics().get(&path).cloned().unwrap_or_default()
}

#[test]
fn env_outside_config_warns_in_app_code() {
    let text = "<?php\n$a = env('APP_KEY');\n$b = env('X', 1);\n";
    let found = env_snippets("/project/app/Services/Mailer.php", "php", text);
    assert_eq!(found.iter().map(|(code, _)| *code).collect::<Vec<_>>(), ["env-outside-config"; 2]);
    assert_eq!(found[0].1, "env('APP_KEY')");
    assert_eq!(found[1].1, "env('X', 1)");

    let host = MockHost::install();
    let found = env_problems_of(&host, "app/Services/Mailer.php", "<?php\n$a = env('APP_KEY');\n");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Warning);
}

#[test]
fn env_in_config_bootstrap_and_tests_is_quiet() {
    let text = "<?php\n$a = env('APP_KEY');\n";
    for path in ["/project/config/app.php", "/project/bootstrap/app.php", "/project/vendor/a/b.php", "/project/tests/Feature/AExampleTest.php"] {
        assert!(env_snippets(path, "php", text).is_empty(), "{path}");
    }
    assert!(env_snippets("/project/resources/views/a.blade.php", "blade", text).is_empty());
}

#[test]
fn env_method_and_other_functions_named_like_env_are_quiet() {
    let text = "<?php\n$a = $this->env('X');\n$b = getenv('X');\n$c = app_env('X');\n$d = Foo::env('X');\nfunction env($k) {}\n$e = $env('X');\n$f = env;\n";
    assert!(env_snippets("/project/app/A.php", "php", text).is_empty());
}

#[test]
fn env_in_comments_is_quiet() {
    let text = "<?php\n// env('A')\n# env('B')\n/* env('C') */\n";
    assert!(env_snippets("/project/app/A.php", "php", text).is_empty());
}

#[test]
fn env_in_strings_and_heredocs_is_quiet() {
    let text = "<?php\n$m = \"use env('X') here\"; $n = 'env(1)'; $o = 'it\\'s env(2)';\n$h = <<<TXT\nCall env('Y') instead\nTXT;\n$r = env('REAL');\n";
    let found = env_snippets("/project/app/A.php", "php", text);
    assert_eq!(found, [("env-outside-config", "env('REAL')".into())]);
}

#[test]
fn env_with_a_leading_backslash_is_a_real_call() {
    let found = env_snippets("/project/app/A.php", "php", "<?php\n$a = \\env('X');\n");
    assert_eq!(found, [("env-outside-config", "env('X')".into())]);
}

#[test]
fn nested_env_calls_report_only_the_outer_call() {
    let found = env_snippets("/project/app/A.php", "php", "<?php\n$a = env('A', env('B'));\n");
    assert_eq!(found, [("env-outside-config", "env('A', env('B'))".into())]);
}

#[test]
fn env_check_can_be_switched_off() {
    let host = MockHost::install();
    host.set_setting("checkFunctions", "false");
    assert!(env_problems_of(&host, "app/A.php", "<?php\n$a = env('APP_KEY');\n").is_empty());
}

#[test]
fn env_check_ranges_use_utf16_columns_with_multibyte_text() {
    let host = MockHost::install();
    let found = env_problems_of(&host, "app/A.php", "<?php\n// é日本😀\n$x = 'é日'; env('A');\n");
    let range = found[0].range;
    assert_eq!((range.start.line, range.start.character, range.end.character), (2, 11, 19));
}

#[test]
fn env_check_crlf_files_report_correct_lines() {
    let host = MockHost::install();
    let found = env_problems_of(&host, "app/A.php", "<?php\r\n\r\n$x = env('A');\r\n");
    assert_eq!((found[0].range.start.line, found[0].range.start.character), (2, 5));
}

#[test]
fn env_check_fixed_file_publishes_empty_list() {
    let host = MockHost::install();
    host.add_file("app/A.php", "<?php\n$x = env('A');\n");
    let mut plugin = activated(&host);
    plugin.handle_events(&[Event::FileOpened("/project/app/A.php".into())], &host.invocation());
    assert_eq!(host.diagnostics()["/project/app/A.php"].len(), 1);

    host.add_file("app/A.php", "<?php\n$x = config('a.b');\n");
    plugin.handle_events(&[Event::FileSaved("/project/app/A.php".into())], &host.invocation());
    assert!(host.diagnostics().get("/project/app/A.php").is_none_or(Vec::is_empty));
}

fn order_by_fix(text: &str, marker: &str) -> String {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    apply_rewrite(&mut plugin, "/project/app/A.php", "php", text, marker)
}

#[test]
fn order_by_updated_at_desc_suggests_latest_with_the_column() {
    let text = "<?php\n$a = Post::orderBy('updated_at', 'desc')->get();\n";
    assert_eq!(order_by_fix(text, "orderBy('updated_at'"), "<?php\n$a = Post::latest('updated_at')->get();\n");
}

#[test]
fn order_by_published_at_asc_suggests_oldest_with_the_column() {
    let text = "<?php\n$a = Post::orderBy('published_at', 'asc')->get();\n$b = Post::orderBy('date')->get();\n$c = Post::orderBy('due_date', 'desc')->get();\n";
    assert_eq!(order_by_fix(text, "orderBy('published_at'"), text.replace("orderBy('published_at', 'asc')", "oldest('published_at')"));
    assert_eq!(order_by_fix(text, "orderBy('date')"), text.replace("orderBy('date')", "oldest('date')"));
    assert_eq!(order_by_fix(text, "orderBy('due_date'"), text.replace("orderBy('due_date', 'desc')", "latest('due_date')"));
}

#[test]
fn order_by_non_date_column_is_quiet() {
    static NOTHING: [(&str, &str); 0] = [];
    let text = "<?php\n$a = Post::orderBy('title', 'desc');\n$b = Post::orderBy('attendance');\n$c = Post::orderBy('updated_at_x');\n$d = Post::orderBy('_at');\n";
    assert!(eloquent_snippets(text, &NOTHING).is_empty());
}

#[test]
fn order_by_keeps_double_quotes() {
    let text = "<?php\n$a = Post::orderBy(\"updated_at\", \"desc\")->get();\n";
    assert_eq!(order_by_fix(text, "orderBy(\"updated_at\""), "<?php\n$a = Post::latest(\"updated_at\")->get();\n");
}
