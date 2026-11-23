use super::*;
use openidea_plugin::mock::MockHost;

/// Exercise the same bounded steps as editor callbacks until fixtures are ready.
pub(crate) fn finish_indexing(plugin: &mut Main) {
    for _ in 0..10000 {
        if plugin.pending_indexes.is_empty() && plugin.index_rechecks.is_empty() {
            for project in host::projects().unwrap_or_default() {
                if project.root().ok().is_some_and(|root|plugin.helper_automation.has_deferred(&root)) { plugin.advance_index(&project); }
            }
            return;
        }
        let projects = host::projects().unwrap_or_default();
        for project in projects { plugin.advance_index(&project); }
    }
    panic!("cooperative index did not finish: {:?}", plugin.index_progress);
}


fn laravel_project(host: &MockHost) {
    host.add_file("composer.json", r#"{"require": {"laravel/framework": "^11.0"},"autoload":{"psr-4":{"App\\":"app/"}}}"#);
    host.add_file(
        "routes/web.php",
        "<?php\nuse App\\Http\\Controllers\\PostController;\n\nRoute::get('/', fn () => view('welcome'))->name('home');\nRoute::resource('posts', PostController::class)->only(['index', 'show']);\nRoute::prefix('admin')->name('admin.')->group(function () {\n    Route::get('/dashboard', [PostController::class, 'index'])->name('dashboard');\n});\n",
    );
    host.add_file("app/Http/Controllers/Controller.php","<?php namespace App\\Http\\Controllers; abstract class Controller {}\n");
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
    host.add_file("app/Models/Post.php", "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nclass Post extends Model\n{\n    protected $fillable = ['title'];\n}\n");
    host.add_file("app/Models/Article.php", "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nclass Article extends Model\n{\n    protected $table = 'posts';\n}\n");
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

pub(super) fn activated(host: &MockHost) -> Main {
    laravel_project(host);
    let mut plugin = Main::default();
    plugin.activate(&host.context("aritti.laravel")).unwrap();crate::tests::finish_indexing(&mut plugin);
    plugin
}

pub(super) fn doc(path: &str, language: &str, text: &str) -> Document {
    Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
}

fn labels(items: Vec<CompletionItem>) -> Vec<String> {
    items.into_iter().map(|item| item.label).collect()
}

/// The text without its `|` and the position the `|` marked.
pub(super) fn cursor(marked: &str) -> (String, Position) {
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
    let completion_source="<?php Route::get('/a', 'Post');";
    let candidate=plugin.completion(&doc("/project/routes/api.php","php",completion_source),text::position(completion_source,completion_source.find("Post").unwrap()+4),None).into_iter().find(|item|item.label=="PostController@show").unwrap();
    assert_eq!(candidate.insert_text,"\\App\\Http\\Controllers\\PostController@show");

    let source="<?php\nRoute::get('/a', 'App\\Http\\Controllers\\PostController@show');\n";
    let method = plugin.definition(&doc("/project/routes/api.php", "php", source), text::position(source,source.find("@show").unwrap()+1));
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
    assert_eq!(complete(controller, "<?php $query = Post::query()->latest();\n$query->where('|');"), columns);
    assert!(complete(controller, "<?php $query = collect([]);\n$query->where('|');").is_empty());

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
fn live_inventory_problems_are_separate_from_manual_opened_file_checks() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    host.add_file("resources/views/broken.blade.php", "@include('missing.partial')\n");
    plugin.handle_events(&[Event::FileOpened("/project/resources/views/broken.blade.php".into())], &host.invocation());crate::tests::finish_indexing(&mut plugin);
    assert!(host.diagnostics().get("/project/resources/views/broken.blade.php").is_none_or(Vec::is_empty));
    host.open_editor("/project/resources/views/broken.blade.php", "blade", "@include('missing.partial')\n", &[]);
    host.refresh_diagnostics(&mut plugin);
    assert_eq!(host.provider_diagnostics()["/project/resources/views/broken.blade.php"].len(), 1);
}

#[test]
fn marks_routes_views_and_layouts() {
    let host = MockHost::install();
    let mut plugin = activated(&host);

    let controller = host_text(&host, "app/Http/Controllers/PostController.php");
    let markers = plugin.line_markers(&doc("/project/app/Http/Controllers/PostController.php", "php", &controller));
    let tooltips: Vec<_> = markers.iter().map(|m| (m.line, m.tooltip.as_str())).collect();
    assert_eq!(tooltips[..4], [(7, "View posts.index"), (5, "Route GET|HEAD /posts"), (5, "Route GET|HEAD /admin/dashboard"), (10, "Route GET|HEAD /posts/{post}")]);
    let related: Vec<_> = markers.iter().filter(|m| m.tooltip == "Related routes for App\\Http\\Controllers\\PostController").collect();
    assert_eq!(related.len(), 2, "retain the class-level route links formerly supplied by the host index");
    assert!(related.iter().all(|m| m.line == 3 && m.target.as_ref().is_some_and(|l| l.path.ends_with("routes/web.php"))));

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

    let View { body: Body::Other { kind, data }, .. } = plugin.render("laravel-views", None, &ctx).unwrap() else { panic!("not a views table") };
    assert_eq!(kind,"resources");
    let table:serde_json::Value=serde_json::from_str(&data).unwrap();let items=table["items"].as_array().unwrap();
    assert_eq!(items.len(), 7);
    assert!(items.iter().all(|item| item["key"].as_str().unwrap().starts_with("view:")));

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
        assert_eq!(args, ["artisan", "cache:clear", "--no-interaction", "--no-ansi"]);
        Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: "ok".into(), stderr: String::new() })
    });
    assert_eq!(plugin.run_command("cache-clear", &host.invocation(), None).unwrap(), None);
    let notes: Vec<_> = host.notifications().into_iter().map(|n| (n.title, n.body)).collect();
    assert_eq!(notes, [("Laravel".into(), "Ran artisan cache:clear. Project indexing is queued.".into())]);
}

#[test]
fn artisan_denied_names_permission() {
    let host = MockHost::install().with_capabilities(&["project.read", "events"]);
    let mut plugin = activated(&host);
    let error = plugin.run_command("cache-clear", &host.invocation(), None).unwrap_err().to_string();
    assert!(error.contains("needs the process:php permission"), "{error}");
    assert!(error.contains("Settings › Plugins › Laravel"), "{error}");
    assert!(error.contains("php is on your PATH"), "{error}");
}

#[test]
fn artisan_failure_keeps_the_cause_from_both_streams_without_terminal_formatting() {
    let host = MockHost::install().with_capabilities(&["project.read", "events", "process:php"]);
    let mut plugin = activated(&host);
    host.on_process("php", |args, _| {
        assert_eq!(args, ["artisan", "optimize:clear", "--no-interaction", "--no-ansi"]);
        Ok(openidea_plugin::ProcessOutput {
            status: Some(1),
            stdout: "\x1b[31mSQLSTATE[HY000]: Connection refused\x1b[0m".into(),
            stderr: "\x1b[2m+47 vendor frames\x1b[22m\n48 artisan:16".into(),
        })
    });
    let error = plugin.run_command("optimize-clear", &host.invocation(), None).unwrap_err().to_string();
    assert!(error.contains("SQLSTATE[HY000]: Connection refused"));
    assert!(error.contains("exit 1"));
    assert!(!error.contains('\x1b'));
    assert!(host.notifications().is_empty(), "a failed command must not emit the success notification");
}

#[test]
fn ignores_projects_without_laravel() {
    let host = MockHost::install();
    host.add_file("composer.json", r#"{"require": {"symfony/framework-bundle": "^7.0"}}"#);
    let mut plugin = Main::default();
    plugin.activate(&host.context("aritti.laravel")).unwrap();crate::tests::finish_indexing(&mut plugin);
    assert_eq!(plugin.run_command("reindex", &host.invocation(), None).unwrap(), None);
    assert_eq!(host.notifications()[0].body, "Indexing Laravel project…");
    crate::tests::finish_indexing(&mut plugin);
    assert_eq!(plugin.summary(), "No Laravel project is open.");
}

fn activated_with(host: &MockHost, files: &[(&str, &str)]) -> Main {
    laravel_project(host);
    for (path, text) in files {
        host.add_file(path, text);
    }
    let mut plugin = Main::default();
    plugin.activate(&host.context("aritti.laravel")).unwrap();crate::tests::finish_indexing(&mut plugin);
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
fn explicitly_configured_empty_inertia_roots_report_missing_pages() {
    let host=MockHost::install();
    let plugin=activated_with(&host,&[("ide.json",r#"{"packages":{"inertia":{"ignoreSettingsPaths":true,"pagePaths":[{"path":"/frontend/screens","prefix":"Admin/"}]}}}"#)]);
    assert_eq!(codes(&plugin,"php","<?php Inertia::render('Admin/Missing');"),["laravel.unknown-inertia-page"]);
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
    assert_eq!(paths.len(), 47, "{paths:?}");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in paths {
        let body = std::fs::read_to_string(root.join(path)).unwrap_or_else(|_| panic!("missing {path}"));
        if path.ends_with("php") && !path.ends_with("blade.php") {
            assert!(body.starts_with("<?php"), "{path}");
            assert!(path.ends_with("blade.php") || !body.contains("{{"), "{path}");
        }
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
        assert_eq!(args, ["artisan", "migrate:fresh", "--no-interaction", "--no-ansi"]);
        Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: "ok".into(), stderr: String::new() })
    });
    host.answer_next_prompt(None);
    plugin.run_command("migrate-fresh", &host.invocation(), None).unwrap();
    assert!(host.process_runs().is_empty());
    assert!(host.notifications().is_empty());
    host.answer_next_prompt(Some(&["yes"]));
    plugin.run_command("migrate-fresh", &host.invocation(), None).unwrap();
    assert_eq!(host.process_runs().len(), 1);
}

#[test]
fn seeding_runs_only_when_confirmed() {
    let host = MockHost::install().with_capabilities(&["project.read", "events", "process:php"]);
    let mut plugin = activated(&host);
    host.on_process("php", |args, _| {
        assert_eq!(args, ["artisan", "db:seed", "--no-interaction", "--no-ansi"]);
        Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: "ok".into(), stderr: String::new() })
    });
    host.answer_next_prompt(None);
    plugin.run_command("db-seed", &host.invocation(), None).unwrap();
    assert!(host.process_runs().is_empty());
    host.answer_next_prompt(Some(&["yes"]));
    plugin.run_command("db-seed", &host.invocation(), None).unwrap();
    assert_eq!(host.process_runs().len(), 1);
}

#[test]
fn database_confirmation_names_the_project_connection_and_environment() {
    let host = MockHost::install();
    laravel_project(&host);
    let project = host.invocation().project().unwrap();
    assert_eq!(crate::database_command_prompt(&project, "test.body", "Seed {root}?").unwrap(), "Seed /project?");
    host.add_file(".env", "# local\nAPP_ENV=\"staging\"\nDB_CONNECTION=pgsql\n");
    assert_eq!(
        crate::database_command_prompt(&project, "test.body", "Seed {root}?").unwrap(),
        "Seed /project?\nDatabase connection: pgsql. Environment: staging."
    );
}

#[test]
fn route_commands_from_the_palette_ask_which_route() {
    let host = MockHost::install();
    let mut plugin = activated(&host);

    host.answer_next_prompt(Some(&["route:1"]));
    host.answer_next_prompt(Some(&["http://localhost:8000"]));
    plugin.run_command("route-copy-curl", &host.invocation(), None).unwrap();
    assert_eq!(host.clipboard().as_deref(), Some("curl -X GET 'http://localhost:8000/posts'"));

    host.answer_next_prompt(None);
    assert_eq!(plugin.run_command("route-send-request", &host.invocation(), None).unwrap(), None);
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

const UNGUARDED_POST: (&str, &str) = ("app/Models/Post.php", "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nclass Post extends Model\n{\n    protected $guarded = [];\n}\n");
const FILLABLE_POST: (&str, &str) = ("app/Models/Post.php", "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nclass Post extends Model\n{\n    protected $fillable = ['title', 'body'];\n}\n");
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

    static CUSTOM: [(&str, &str); 1] = [("app/Models/Post.php", "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nclass Post extends Model\n{\n    protected $fillable = ['a'];\n    public function getFillable() {}\n}\n")];
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

    plugin.handle_events(&[Event::FileOpened(path.into())], &host.invocation());crate::tests::finish_indexing(&mut plugin);
    let found: Vec<_> = host.diagnostics()[path].iter().filter(|d| d.code.as_deref() == Some("guarded-attribute-assignment")).cloned().collect();
    assert_eq!(found.len(), 1);

    host.add_file("app/Http/Controllers/C.php", "<?php\nnamespace App\\Http;\nuse App\\Models\\Post;\nPost::create($request->validated());\n");
    plugin.handle_events(&[Event::FileOpened(path.into())], &host.invocation());crate::tests::finish_indexing(&mut plugin);
    assert!(host.diagnostics().get(path).is_none_or(Vec::is_empty));

    host.add_file("app/Http/Controllers/C.php", "<?php\nnamespace App\\Http;\nuse App\\Models\\Post;\nPost::create($request->all());\n");
    host.set_setting("checkEloquent", "false");
    host.set_setting("checkSecurity", "false");
    plugin.handle_events(&[Event::FileOpened(path.into())], &host.invocation());crate::tests::finish_indexing(&mut plugin);
    assert!(host.diagnostics().get(path).is_none_or(Vec::is_empty));
}

fn guarded_mass_assignment_in(relative: &str) -> Vec<Diagnostic> {
    let host = MockHost::install();
    let body = "<?php\nnamespace Database;\nuse App\\Models\\Post;\nPost::create($request->all());\n";
    let mut plugin = activated_with(&host, &[("app/Models/Post.php", UNGUARDED_POST.1), (relative, body)]);
    let path = format!("/project/{relative}");

    plugin.handle_events(&[Event::FileOpened(path.clone())], &host.invocation());crate::tests::finish_indexing(&mut plugin);
    host.diagnostics().get(&path).cloned().unwrap_or_default().into_iter().filter(|d| d.code.as_deref() == Some("guarded-attribute-assignment")).collect()
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

    plugin.handle_events(&[Event::FileOpened(path.into())], &host.invocation());crate::tests::finish_indexing(&mut plugin);
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
    plugin.activate(&host.context("aritti.laravel")).unwrap();crate::tests::finish_indexing(&mut plugin);

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
    plugin.activate(&host.context("aritti.laravel")).unwrap();crate::tests::finish_indexing(&mut plugin);

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
    plugin.activate(&host.context("aritti.laravel")).unwrap();crate::tests::finish_indexing(&mut plugin);

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

    plugin.handle_events(&[Event::FileOpened(path.clone())], &host.invocation());crate::tests::finish_indexing(&mut plugin);
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
fn env_check_survives_non_ascii_text_and_still_warns_after_it() {
    let text = "<?php\n$pr\u{e9}nom = 1;\n?>\n<p>caf\u{e9} \u{20ac} \u{1F600}</p>\n<?php\n$a = env('APP_KEY');\n";
    let found = env_snippets("app/Views/a.php", "php", text);
    assert_eq!(found, [("env-outside-config", "env('APP_KEY')".to_owned())]);
}

#[test]
fn env_quiet_directories_match_the_project_relative_path() {
    let text = "<?php\n$a = env('APP_KEY');\n";
    assert_eq!(env_snippets("app/Services/A.php", "php", text).len(), 1);

    for path in ["config/app.php", "bootstrap/app.php", "vendor/a/b.php", "tests/Feature/AExampleTest.php"] {
        assert!(env_snippets(path, "php", text).is_empty(), "{path}");
    }
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
fn namespaced_env_functions_are_quiet() {
    assert!(env_snippets("/project/app/A.php", "php", "<?php\n$a = Ns\\env('X');\n$b = \\Ns\\env('X');\n").is_empty());
}

#[test]
fn heredoc_does_not_end_at_a_longer_identifier() {
    let text = "<?php\n$h = <<<TXT\nTXTX env('A')\nTXT;\n$r = env('REAL');\n";
    assert_eq!(env_snippets("/project/app/A.php", "php", text), [("env-outside-config", "env('REAL')".into())]);
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
    plugin.handle_events(&[Event::FileOpened("/project/app/A.php".into())], &host.invocation());crate::tests::finish_indexing(&mut plugin);
    assert_eq!(host.diagnostics()["/project/app/A.php"].len(), 1);

    host.add_file("app/A.php", "<?php\n$x = config('a.b');\n");
    plugin.handle_events(&[Event::FileSaved("/project/app/A.php".into())], &host.invocation());crate::tests::finish_indexing(&mut plugin);
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

fn apply_titled(plugin: &mut Main, path: &str, language: &str, text: &str, marker: &str, title: &str) -> String {
    let at = text.find(marker).unwrap() + 1;
    let position = text::position(text, at);
    let action = plugin.code_actions(&doc(path, language, text), Range::new(position, position)).into_iter().find(|a| a.title == title).unwrap_or_else(|| panic!("no action {title}"));
    let mut edits: Vec<_> = action.edits[0].1.iter().map(|e| (text::offset(text, e.range.start).unwrap(), text::offset(text, e.range.end).unwrap(), e.text.clone())).collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut result = text.to_owned();
    for (start, end, replacement) in edits {
        result.replace_range(start..end, &replacement);
    }
    result
}

const NEW_BLADE: &str = "/project/resources/views/a.blade.php";
const NEW_RULES: &str = "/project/app/A.php";

#[test]
fn static_class_attribute_converts_to_class_directive() {
    let host = MockHost::install();
    host.add_file("composer.lock",r#"{"packages":[{"name":"laravel/framework","version":"v11.0.0"}]}"#);
    let mut plugin = activated(&host);
    let text = "<div class=\"btn btn-primary\">x</div>\n";
    let result = apply_titled(&mut plugin, NEW_BLADE, "blade", text, "btn-primary", "Convert class attribute to @class");
    assert_eq!(result, "<div @class(['btn', 'btn-primary'])>x</div>\n");
}

#[test]
fn class_attribute_with_echo_wraps_into_class_directive() {
    let host = MockHost::install();
    host.add_file("composer.lock",r#"{"packages":[{"name":"laravel/framework","version":"v11.0.0"}]}"#);
    let mut plugin = activated(&host);
    let text = "<div id=\"a\" class=\"btn {{ $size }}\">x</div>\n";
    let result = apply_titled(&mut plugin, NEW_BLADE, "blade", text, "btn {{", "Wrap class attribute in @class");
    assert_eq!(result, "<div id=\"a\" @class(['btn', $size])>x</div>\n");
}

#[test]
fn class_directive_is_not_offered_for_conditionals_unescaped_echo_or_existing_directive() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    for text in [
        "<div class=\"btn @if($a) on @endif\">x</div>\n",
        "<div class=\"btn {!! $size !!}\">x</div>\n",
        "<div @class(['a']) class=\"btn\">x</div>\n",
        "<div class=\"\">x</div>\n",
        "<div class=\"it's\">x</div>\n",
        "<div class=\"btn-{{ $size }}\">x</div>\n",
    ] {
        let titles = rewrite_titles(&mut plugin, NEW_BLADE, "blade", text, "class=");
        assert!(titles.iter().all(|t| !t.contains("@class")), "{text}");
    }
    let php = "<?php\n$x = '<div class=\"btn\">';\n";
    assert!(rewrite_titles(&mut plugin, "/project/app/A.php", "php", php, "btn").is_empty());
}

#[test]
fn all_rule_strings_in_an_array_convert_together() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\n$request->validate(['a' => 'required|email', 'b' => 'max:5|min:1', 'c' => 'required']);\n";
    let result = apply_titled(&mut plugin, NEW_RULES, "php", text, "required|email", "Convert all validation rules in this array to arrays");
    assert_eq!(result, "<?php\n$request->validate(['a' => ['required', 'email'], 'b' => ['max:5', 'min:1'], 'c' => 'required']);\n");
}

#[test]
fn all_rules_is_not_offered_for_a_single_convertible_item() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\n$request->validate(['a' => 'required|email', 'b' => 'required', 'c' => ['x', 'y']]);\n";
    let titles = rewrite_titles(&mut plugin, NEW_RULES, "php", text, "required|email");
    assert!(!titles.contains(&"Convert all validation rules in this array to arrays".to_owned()));
}

#[test]
fn all_rules_leaves_regex_and_non_convertible_items_alone() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\n$request->validate(['a' => 'required|email', 'r' => 'regex:/a|b/', 'b' => 'max:5|min:1', 'n' => ['x', 'y|z'], 'o' => Rule::in(['a'])]);\n";
    let result = apply_titled(&mut plugin, NEW_RULES, "php", text, "required|email", "Convert all validation rules in this array to arrays");
    assert_eq!(
        result,
        "<?php\n$request->validate(['a' => ['required', 'email'], 'r' => 'regex:/a|b/', 'b' => ['max:5', 'min:1'], 'n' => ['x', 'y|z'], 'o' => Rule::in(['a'])]);\n"
    );
}

const NEW_ROUTES: &str = "/project/routes/web.php";

#[test]
fn all_controller_strings_convert_together() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\nuse App\\Http\\Controllers\\UserController;\nuse App\\Http\\Controllers\\PostController;\nRoute::get('/u', 'UserController@index');\nRoute::post('/p', 'PostController@store');\nRoute::get('/x', 'Missing@index');\n";
    let result = apply_titled(&mut plugin, NEW_ROUTES, "php", text, "UserController@", "Convert all controller strings in this file");
    assert_eq!(
        result,
        "<?php\nuse App\\Http\\Controllers\\UserController;\nuse App\\Http\\Controllers\\PostController;\nRoute::get('/u', [UserController::class, 'index']);\nRoute::post('/p', [PostController::class, 'store']);\nRoute::get('/x', 'Missing@index');\n"
    );
}

#[test]
fn resource_controller_string_converts_to_class_constant() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\nuse App\\Http\\Controllers\\PhotoController;\nuse App\\Http\\Controllers\\UserController;\nRoute::resource('photos', 'PhotoController');\nRoute::get('/u', 'UserController@index');\n";
    let result = apply_titled(&mut plugin, NEW_ROUTES, "php", text, "PhotoController'", "Convert controller string to ::class");
    assert!(result.contains("Route::resource('photos', PhotoController::class);"));
    let all = apply_titled(&mut plugin, NEW_ROUTES, "php", text, "PhotoController'", "Convert all controller strings in this file");
    assert!(all.contains("Route::resource('photos', PhotoController::class);"));
    assert!(all.contains("[UserController::class, 'index']"));
}

#[test]
fn all_controllers_is_quiet_for_unresolvable_names() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\nuse App\\Http\\Controllers\\UserController;\nRoute::get('/u', 'UserController@index');\nRoute::get('/x', 'Other@index');\nRoute::resource('p', 'Gone');\n";
    let titles = rewrite_titles(&mut plugin, NEW_ROUTES, "php", text, "UserController@");
    assert!(!titles.contains(&"Convert all controller strings in this file".to_owned()));
}

#[test]
fn controllers_namespace_setting_is_used() {
    let host = MockHost::install();
    host.set_setting("controllersNamespace", r#""App\\Http\\Controllers\\Api""#);
    let mut plugin = activated(&host);
    let text = "<?php\nnamespace App\\Http\\Controllers\\Api;\nRoute::get('/u', 'UserController@index');\nRoute::get('/v', 'PostController@index');\n";
    assert_eq!(rewrite_titles(&mut plugin, NEW_ROUTES, "php", text, "UserController@").len(), 2);

    let default = "<?php\nnamespace App\\Http\\Controllers;\nRoute::get('/u', 'UserController@index');\n";
    assert!(rewrite_titles(&mut plugin, NEW_ROUTES, "php", default, "UserController@").is_empty());
}

#[test]
fn rule_object_replaces_exists_unique_in_and_not_in() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let base = "<?php\nnamespace App\\Http\\Requests;\n\nuse Foo\\Bar;\n\n$request->validate(['f' => ['required', 'EXPR']]);\n";
    let cases = [
        ("exists:users,id", "Convert to Rule::exists('users', 'id')", "Rule::exists('users', 'id')"),
        ("unique:users,email", "Convert to Rule::unique('users', 'email')", "Rule::unique('users', 'email')"),
        ("exists:users", "Convert to Rule::exists('users')", "Rule::exists('users')"),
        ("in:a,b", "Convert to Rule::in(['a', 'b'])", "Rule::in(['a', 'b'])"),
        ("not_in:a,b", "Convert to Rule::notIn(['a', 'b'])", "Rule::notIn(['a', 'b'])"),
    ];
    for (rule, title, call) in cases {
        let text = base.replace("EXPR", rule);
        let result = apply_titled(&mut plugin, NEW_RULES, "php", &text, rule, title);
        assert_eq!(result, "<?php\nnamespace App\\Http\\Requests;\n\nuse Foo\\Bar;\nuse Illuminate\\Validation\\Rule;\n\n$request->validate(['f' => ['required', EXPR]]);\n".replace("EXPR", call));
    }
}

#[test]
fn rule_object_is_not_offered_for_extra_arguments_or_pipe_strings() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    for text in [
        "<?php\n$request->validate(['f' => ['required', 'unique:users,email,5']]);\n",
        "<?php\n$request->validate(['f' => ['required', 'exists:users,id|max:5']]);\n",
        "<?php\n$request->validate(['f' => 'exists:users,id']);\n",
        "<?php\n$request->validate(['f' => ['required', 'max:5']]);\n",
    ] {
        let titles: Vec<String> = ["required", "users", "max:5"].into_iter().filter(|m| text.contains(m)).flat_map(|m| rewrite_titles(&mut plugin, NEW_RULES, "php", text, m)).collect();
        assert!(titles.iter().all(|t| !t.starts_with("Convert to Rule::")), "{text}");
    }
}

#[test]
fn rule_object_is_not_offered_when_another_rule_class_is_imported() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\nuse Acme\\Rule;\n\n$request->validate(['f' => ['required', 'in:a,b']]);\n";
    assert!(rewrite_titles(&mut plugin, NEW_RULES, "php", text, "in:a").iter().all(|t| !t.starts_with("Convert to Rule::")));
}

#[test]
fn rule_object_reuses_an_existing_import() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\nuse Illuminate\\Validation\\Rule;\n\n$request->validate(['f' => ['required', 'in:a,b']]);\n";
    let result = apply_titled(&mut plugin, NEW_RULES, "php", text, "in:a", "Convert to Rule::in(['a', 'b'])");
    assert_eq!(result, "<?php\nuse Illuminate\\Validation\\Rule;\n\n$request->validate(['f' => ['required', Rule::in(['a', 'b'])]]);\n");
}

#[test]
fn root_facade_adds_the_import() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\nnamespace App;\n\nclass A\n{\n    function f() { return Route::get('/'); }\n}\n";
    let result = apply_titled(&mut plugin, NEW_RULES, "php", text, "Route::", "Import Illuminate\\Support\\Facades\\Route");
    assert_eq!(result, "<?php\nnamespace App;\n\nuse Illuminate\\Support\\Facades\\Route;\n\nclass A\n{\n    function f() { return Route::get('/'); }\n}\n");

    let text = "<?php\nnamespace App;\n\nclass A { function f() { return Str::of('a'); } }\n";
    let result = apply_titled(&mut plugin, NEW_RULES, "php", text, "Str::", "Import Illuminate\\Support\\Str");
    assert!(result.contains("use Illuminate\\Support\\Str;"));
}

#[test]
fn root_facade_import_is_quiet_when_the_name_is_imported_elsewhere() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    for import in ["Illuminate\\Support\\Facades\\Route", "Acme\\Route", "Acme\\Thing as Route"] {
        let text = format!("<?php\nnamespace App;\n\nuse {import};\n\nclass A {{ function f() {{ return Route::get('/'); }} }}\n");
        assert!(rewrite_titles(&mut plugin, NEW_RULES, "php", &text, "Route::get").is_empty(), "{import}");
    }
}

#[test]
fn root_facade_is_quiet_without_a_namespace_or_for_unknown_aliases() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    assert!(rewrite_titles(&mut plugin, NEW_ROUTES, "php", "<?php\nRoute::get('/');\n", "Route::").is_empty());
    let text = "<?php\nnamespace App;\n\nFoo::bar(); \\Route::get('/'); $x->Route::y;\n";
    assert!(rewrite_titles(&mut plugin, NEW_RULES, "php", text, "Foo::").is_empty());
    assert!(rewrite_titles(&mut plugin, NEW_RULES, "php", text, "Route::get").is_empty());
}

#[test]
fn rewrites_applied_twice_do_not_duplicate_imports() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\nnamespace App;\n\n$request->validate(['f' => ['required', 'in:a,b', 'not_in:c']]);\n";
    let once = apply_titled(&mut plugin, NEW_RULES, "php", text, "in:a", "Convert to Rule::in(['a', 'b'])");
    let twice = apply_titled(&mut plugin, NEW_RULES, "php", &once, "not_in:c", "Convert to Rule::notIn(['c'])");
    assert_eq!(twice.matches("use Illuminate\\Validation\\Rule;").count(), 1);
    assert!(twice.contains("Rule::in(['a', 'b'])"));
    assert!(twice.contains("Rule::notIn(['c'])"));

    let facade = apply_titled(&mut plugin, NEW_RULES, "php", "<?php\nnamespace App;\n\nRoute::get('/');\nRoute::post('/');\n", "Route::get", "Import Illuminate\\Support\\Facades\\Route");
    assert!(rewrite_titles(&mut plugin, NEW_RULES, "php", &facade, "Route::post").is_empty());
}

#[test]
fn new_rewrites_survive_multibyte_text_and_crlf() {
    let host = MockHost::install();
    host.add_file("composer.lock",r#"{"packages":[{"name":"laravel/framework","version":"v11.0.0"}]}"#);
    let mut plugin = activated(&host);
    let text = "<?php\r\nnamespace App;\r\n\r\nuse Foo\\Bar;\r\n\r\n// é日本\r\n$request->validate(['f' => ['é', 'in:a,b']]);\r\n";
    let result = apply_titled(&mut plugin, NEW_RULES, "php", text, "in:a", "Convert to Rule::in(['a', 'b'])");
    assert_eq!(result, "<?php\r\nnamespace App;\r\n\r\nuse Foo\\Bar;\r\nuse Illuminate\\Validation\\Rule;\r\n\r\n// é日本\r\n$request->validate(['f' => ['é', Rule::in(['a', 'b'])]]);\r\n");

    let text = "<?php\r\nnamespace App;\r\n\r\n// é日本\r\nRoute::get('é');\r\n";
    let result = apply_titled(&mut plugin, NEW_RULES, "php", text, "Route::", "Import Illuminate\\Support\\Facades\\Route");
    assert_eq!(result, "<?php\r\nnamespace App;\r\n\r\nuse Illuminate\\Support\\Facades\\Route;\r\n\r\n// é日本\r\nRoute::get('é');\r\n");

    let blade = "<p>é日本</p>\r\n<div class=\"a {{ $é }}\">x</div>\r\n";
    let result = apply_titled(&mut plugin, NEW_BLADE, "blade", blade, "a {{", "Wrap class attribute in @class");
    assert_eq!(result, "<p>é日本</p>\r\n<div @class(['a', $é])>x</div>\r\n");

    let rules = "<?php\r\n$request->validate(['é' => 'a|b', 'x' => 'c|d']);\r\n";
    let result = apply_titled(&mut plugin, NEW_RULES, "php", rules, "a|b", "Convert all validation rules in this array to arrays");
    assert_eq!(result, "<?php\r\n$request->validate(['é' => ['a', 'b'], 'x' => ['c', 'd']]);\r\n");
}

#[test]
fn context_facade_imports_from_the_facades_namespace() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\nnamespace App;\n\nContext::add('a', 1);\n";
    let result = apply_titled(&mut plugin, NEW_RULES, "php", text, "Context::", "Import Illuminate\\Support\\Facades\\Context");
    assert!(result.contains("use Illuminate\\Support\\Facades\\Context;"));
}

#[test]
fn root_facade_ignores_longer_class_names_but_not_a_declared_one() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    for (declared, call) in [("AppServiceProvider", "App"), ("RouteServiceProvider", "Route"), ("Authenticate", "Auth"), ("LogController", "Log")] {
        let text = format!("<?php\nnamespace App;\n\nclass {declared}\n{{\n    function f() {{ return {call}::x(); }}\n}}\n");
        let marker = format!("{call}::x");
        assert_eq!(rewrite_titles(&mut plugin, NEW_RULES, "php", &text, &marker).len(), 1, "{declared}");
    }
    for keyword in ["class", "interface", "trait", "enum"] {
        let text = format!("<?php\nnamespace App;\n\n{keyword} Route {{}}\nRoute::x();\n");
        assert!(rewrite_titles(&mut plugin, NEW_RULES, "php", &text, "Route::x").is_empty(), "{keyword}");
    }
}

#[test]
fn rule_object_is_quiet_when_a_rule_class_is_declared_in_the_file() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\nnamespace App;\n\nclass Rule {}\n$request->validate(['f' => ['required', 'in:a,b']]);\n";
    assert!(rewrite_titles(&mut plugin, NEW_RULES, "php", text, "in:a").iter().all(|t| !t.starts_with("Convert to Rule::")));
}

fn views_of(host: &MockHost) -> Vec<String> {
    let plugin = activated(host);
    plugin.index_for("/project/x").unwrap().views.keys().cloned().collect()
}

#[test]
fn ide_blade_json_namespace_indexes_views_as_namespaced_names() {
    let host = MockHost::install();
    host.add_file("ide-blade.json", r#"{"namespaces": [{"namespace": "admin", "path": "/modules/admin/views/"}]}"#);
    host.add_file("modules/admin/views/users/index.blade.php", "<p></p>\n");

    assert!(views_of(&host).contains(&"admin::users.index".to_owned()));
}

#[test]
fn ide_blade_json_entry_without_namespace_adds_a_view_root() {
    let host = MockHost::install();
    host.add_file("ide-blade.json", r#"{"namespaces": [{"namespace": "", "path": "extra/views"}, {"path": "other"}]}"#);
    host.add_file("extra/views/banner.blade.php", "<p></p>\n");
    host.add_file("other/footer.blade.php", "<p></p>\n");

    let views = views_of(&host);
    assert!(views.contains(&"banner".to_owned()) && views.contains(&"footer".to_owned()));
}

#[test]
fn ide_blade_json_entries_without_a_path_are_ignored() {
    assert_eq!(
        index::ide_blade_namespaces(r#"{"namespaces": [{"namespace": "a"}, {"namespace": "b", "path": "  "}, {"namespace": "c", "path": "ok"}]}"#),
        [(Some("c".to_owned()), "ok".to_owned())]
    );
}

#[test]
fn ide_blade_json_strips_a_leading_dot_slash() {
    assert_eq!(
        index::ide_blade_namespaces(r#"{"namespaces": [{"namespace": "m", "path": "./modules/views"}]}"#),
        [(Some("m".to_owned()), "modules/views".to_owned())]
    );
}

#[test]
fn ide_blade_json_nested_values_do_not_drop_the_file() {
    let text = r#"{"namespaces": [{"namespace": "a", "meta": {"x": [1, {"y": "}"}], "z": "],"}, "path": "one"}, {"namespace": "b", "tags": ["p", "q"], "path": "two"}]}"#;
    assert_eq!(index::ide_blade_namespaces(text), [(Some("a".to_owned()), "one".to_owned()), (Some("b".to_owned()), "two".to_owned())]);
}

#[test]
fn ide_blade_json_handles_escaped_quotes_bom_and_trailing_garbage() {
    let text = "\u{feff}{\"namespaces\": [{\"namespace\": \"a\\\"b\", \"skip\": \"x\\\"},y\", \"path\": \"p\"}]} trailing garbage";
    assert_eq!(index::ide_blade_namespaces(text), [(Some("a\"b".to_owned()), "p".to_owned())]);
}

#[test]
fn ide_blade_json_malformed_is_ignored() {
    for text in ["", "{", "[1,2", r#"{"namespaces": [{"namespace": "a", "path": "b""#, r#"{"namespaces": 3}"#, "not json"] {
        assert!(index::ide_blade_namespaces(text).is_empty(), "{text}");
    }

    let host = MockHost::install();
    host.add_file("ide-blade.json", "{ nope");
    assert!(views_of(&host).contains(&"welcome".to_owned()));
}

#[test]
fn ide_blade_json_in_a_hidden_directory_is_read() {
    let host = MockHost::install();
    host.add_file(".idea/ide-blade.json", r#"{"namespaces": [{"namespace": "hidden", "path": "mods"}]}"#);
    host.add_file("mods/a.blade.php", "<p></p>\n");

    assert!(views_of(&host).contains(&"hidden::a".to_owned()));
}

#[test]
fn ide_blade_json_save_triggers_a_reindex() {
    for saved in ["ide-blade.json", ".idea/ide-blade.json"] {
        let host = MockHost::install();
        host.add_file("mods/a.blade.php", "<p></p>\n");
        let mut plugin = activated(&host);
        assert!(!plugin.index_for("/project/x").unwrap().views.contains_key("n::a"));

        host.add_file(saved, r#"{"namespaces": [{"namespace": "n", "path": "mods"}]}"#);
        plugin.handle_events(&[Event::FileSaved(format!("/project/{saved}"))], &host.invocation());crate::tests::finish_indexing(&mut plugin);
        assert!(plugin.index_for("/project/x").unwrap().views.contains_key("n::a"), "{saved}");
    }
}

#[test]
fn missing_file_is_treated_as_absent() {
    for error in [Error::NotFound("x".into()), Error::Invalid("x".into())] {
        let roots = index::ide_blade_roots(|_| Err(error.clone()));
        assert!(roots.is_empty());
    }

    let host = MockHost::install();
    assert!(views_of(&host).contains(&"welcome".to_owned()));
}

fn directive_item(plugin: &mut Main, marked: &str, label: &str) -> CompletionItem {
    let (text, at) = cursor(marked);
    plugin
        .completion(&doc("/project/resources/views/posts/show.blade.php", "blade", &text), at, None)
        .into_iter()
        .find(|item| item.label == label)
        .unwrap()
}

#[test]
fn snippet_items_carry_arguments_and_the_closing_directive() {
    let host = MockHost::install();
    let mut plugin = activated(&host);

    let item = directive_item(&mut plugin, "@i|", "if");

    assert!(item.snippet);
    assert!(item.insert_text.starts_with("if("));
    assert!(item.insert_text.ends_with("@endif"));
    let each = directive_item(&mut plugin, "@fore|", "foreach");
    assert!(each.insert_text.contains("\\$items"));
}

#[test]
fn snippet_items_replace_only_the_typed_name() {
    let host = MockHost::install();
    let mut plugin = activated(&host);

    let item = directive_item(&mut plugin, "<p>\n@fo|\n", "foreach");

    let replace = item.replace.unwrap();
    assert_eq!((replace.start.line, replace.start.character), (1, 1));
    assert_eq!((replace.end.line, replace.end.character), (1, 3));
}

#[test]
fn directives_without_a_snippet_and_custom_directives_stay_name_only() {
    let host = MockHost::install();
    host.add_file("app/Providers/AppServiceProvider.php", "<?php\nBlade::directive('datetime', fn ($e) => $e);\n");
    let mut plugin = activated(&host);

    for (marked, label) in [("@end|", "endif"), ("@dat|", "datetime"), ("@els|", "else")] {
        let item = directive_item(&mut plugin, marked, label);
        assert!(!item.snippet, "{label}");
        assert_eq!(item.insert_text, label, "{label}");
    }
}

#[test]
fn every_snippet_names_a_core_directive() {
    for (name, _) in crate::directives::SNIPPETS {
        assert!(crate::directives::CORE.contains(name), "{name}");
    }
    for name in ["if", "unless", "isset", "empty", "auth", "guest", "env", "production", "section", "push", "pushOnce",
        "prepend", "for", "foreach", "forelse", "while", "switch", "php", "verbatim", "can", "cannot", "canany", "error",
        "component", "slot", "extends", "yield", "include", "includeIf", "includeWhen", "includeUnless", "includeFirst",
        "each", "stack", "props", "class", "style", "csrf", "method", "json", "vite", "lang", "inject", "livewire"]
    {
        assert!(crate::directives::SNIPPETS.iter().any(|(n, _)| *n == name), "no snippet for {name}");
    }
}

#[test]
fn snippet_placeholders_are_balanced() {
    for (name, body) in crate::directives::SNIPPETS {
        assert!(body.matches("$0").count() <= 1, "{name}");
        assert_eq!(body.matches("${").count(), body.matches('}').count(), "{name}");
        assert!(!body.starts_with('@'), "{name}");

        let mut numbers: Vec<u32> = body
            .match_indices("${")
            .map(|(i, _)| body[i + 2..].split(':').next().unwrap().parse().unwrap())
            .collect();
        numbers.sort();
        assert_eq!(numbers, (1..=numbers.len() as u32).collect::<Vec<_>>(), "{name}");
    }
}

#[test]
fn block_snippets_contain_their_closing_directive() {
    let closers = [
        ("if", "endif"), ("unless", "endunless"), ("isset", "endisset"), ("empty", "endempty"), ("auth", "endauth"),
        ("guest", "endguest"), ("env", "endenv"), ("production", "endproduction"), ("section", "endsection"),
        ("push", "endpush"), ("pushOnce", "endPushOnce"), ("prepend", "endprepend"), ("for", "endfor"),
        ("foreach", "endforeach"), ("forelse", "endforelse"), ("while", "endwhile"), ("switch", "endswitch"),
        ("php", "endphp"), ("verbatim", "endverbatim"), ("can", "endcan"), ("cannot", "endcannot"),
        ("canany", "endcanany"), ("error", "enderror"), ("component", "endcomponent"), ("slot", "endslot"),
    ];
    for (name, closer) in closers {
        let (_, body) = crate::directives::SNIPPETS.iter().find(|(n, _)| *n == name).unwrap();
        assert!(body.ends_with(&format!("@{closer}")), "{name}");
    }
}

#[test]
fn forelse_snippet_has_an_empty_branch_before_its_closing_directive() {
    let (_, body) = crate::directives::SNIPPETS.iter().find(|(n, _)| *n == "forelse").unwrap();

    assert!(body.find("@empty").unwrap() < body.find("@endforelse").unwrap());
}

fn terminal_args(_host: &MockHost, plugin: &Main) -> Vec<Vec<String>> {
    plugin.artisan_terminals.commands()
}

#[test]
fn make_asks_then_runs_artisan_in_a_terminal() {
    let host = MockHost::install().with_capabilities(&["project.read", "events", "process:php", "editor.read", "editor.write"]);
    let mut plugin = activated(&host);
    host.answer_next_prompt(Some(&["controller"]));
    host.answer_next_prompt(Some(&["Admin/ReportController"]));
    host.answer_next_prompt(Some(&["--resource", "--api"]));
    host.answer_next_prompt(Some(&["App\\Models\\Post"]));
    plugin.run_command("make", &host.invocation(), None).unwrap();
    assert_eq!(
        terminal_args(&host, &plugin),
        [["artisan", "make:controller", "Admin/ReportController", "--model=App\\Models\\Post", "--resource", "--api"]]
    );
}

#[test]
fn legacy_model_generator_passes_the_flags() {
    let host = MockHost::install().with_capabilities(&["project.read", "events", "process:php", "editor.read", "editor.write"]);
    let plugin = activated(&host);
    host.answer_next_prompt(Some(&["Tag"]));
    host.answer_next_prompt(Some(&["-m", "-f"]));
    generate::make(&plugin.artisan_terminals, &host.invocation(), plugin.view_index(&host.invocation()), Some("model"), "").unwrap();
    assert_eq!(terminal_args(&host, &plugin), [["artisan", "make:model", "Tag", "-m", "-f"]]);
}

#[test]
fn make_rejects_unsafe_names_and_honours_dismissal() {
    let host = MockHost::install().with_capabilities(&["project.read", "events", "process:php", "editor.read", "editor.write"]);
    let plugin = activated(&host);
    for bad in ["Foo; rm -rf /", "--force", "$(id)", "Foo Bar", "..\\x", ""] {
        host.answer_next_prompt(Some(&[bad]));
        assert!(generate::make(&plugin.artisan_terminals, &host.invocation(), plugin.view_index(&host.invocation()), Some("model"), "").is_err(), "{bad}");
    }
    assert!(generate::make(&plugin.artisan_terminals, &host.invocation(), plugin.view_index(&host.invocation()), Some("model"), "").unwrap().is_none());
    host.answer_next_prompt(Some(&["Tag"]));
    assert!(generate::make(&plugin.artisan_terminals, &host.invocation(), plugin.view_index(&host.invocation()), Some("model"), "").unwrap().is_none());
    assert!(host.terminals().is_empty());
}

#[test]
fn generates_for_a_model_with_a_default_name() {
    let host = MockHost::install().with_capabilities(&["project.read", "events", "process:php", "editor.read", "editor.write"]);
    let mut plugin = activated(&host);
    host.answer_next_prompt(Some(&["App\\Models\\Post"]));
    host.answer_next_prompt(Some(&["policy"]));
    host.answer_next_prompt(Some(&["PostPolicy"]));
    plugin.run_command("generate-for-model", &host.invocation(), None).unwrap();
    assert_eq!(terminal_args(&host, &plugin), [["artisan", "make:policy", "PostPolicy", "--model=App\\Models\\Post"]]);

    host.answer_next_prompt(Some(&["Nope"]));
    assert!(plugin.run_command("generate-for-model", &host.invocation(), None).is_err());
}

#[test]
fn run_artisan_uses_static_choices_without_implicit_discovery_and_splits_arguments() {
    let host = MockHost::install().with_capabilities(&["project.read", "events", "process:php", "editor.read", "editor.write"]);
    let mut plugin = activated(&host);
    host.on_process("php", |_, _| {
        Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: "about  Display info\nmigrate:status  Show status\n".into(), stderr: String::new() })
    });
    host.answer_next_prompt(Some(&["migrate:status"]));
    host.answer_next_prompt(Some(&["--pending  --no-ansi"]));
    plugin.run_command("artisan-run-terminal", &host.invocation(), None).unwrap();
    assert_eq!(terminal_args(&host, &plugin), [["artisan", "migrate:status", "--pending", "--no-ansi"]]);
    assert!(host.process_runs().is_empty(), "opening the command picker must not execute discovery");

    host.answer_next_prompt(Some(&["rm"]));
    assert!(plugin.run_command("artisan-run-terminal", &host.invocation(), None).is_err());
    assert!(plugin.run_command("artisan-run-terminal", &host.invocation(), None).unwrap().is_none());
}

#[test]
fn missing_view_command_creates_the_view_after_confirming_without_artisan() {
    let host = MockHost::install().with_capabilities(&["project.read", "project.write", "events", "process:php", "editor.read", "editor.write"]);
    host.add_dir("resources/views");
    let mut plugin = activated(&host);
    let text = "<?php\nreturn view('posts.create');\n";
    host.add_file("app/Http/Controllers/A.php", text);
    host.open_editor("/project/app/Http/Controllers/A.php", "php", text, &[(1, 17, 1, 17)]);
    host.answer_next_prompt(None);
    plugin.run_command("create-missing", &host.invocation(), None).unwrap();
    let project = host.invocation().project().unwrap();
    assert!(!project.exists("resources/views/posts/create.blade.php").unwrap(), "declining the confirm writes nothing");
    host.answer_next_prompt(Some(&[]));
    plugin.run_command("create-missing", &host.invocation(), None).unwrap();
    assert!(project.exists("resources/views/posts/create.blade.php").unwrap());
    assert!(host.terminals().is_empty());

    let existing = "<?php\nreturn view('welcome');\n";
    host.open_editor("/project/app/Http/Controllers/A.php", "php", existing, &[(1, 17, 1, 17)]);
    assert!(plugin.run_command("create-missing", &host.invocation(), None).is_err());
}

#[test]
fn find_unused_views_opens_the_chosen_one() {
    let host = MockHost::install();
    host.add_file("resources/views/orphan.blade.php", "<p>x</p>\n");
    let mut plugin = activated(&host);
    host.answer_next_prompt(Some(&["orphan"]));
    plugin.run_command("unused-views", &host.invocation(), None).unwrap();
    let opened = host.opened_locations();
    assert_eq!(opened.len(), 1);
    assert!(opened[0].0.ends_with("resources/views/orphan.blade.php"));
}

#[test]
fn find_unused_views_says_so_when_all_are_used() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    plugin.run_command("unused-views", &host.invocation(), None).unwrap();
    assert!(host.opened_locations().is_empty());
    assert!(host.notifications().iter().any(|n| n.body.contains("Every view is used")));
}

#[test]
fn model_phpdoc_removal_is_offered_inside_the_block() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\n\n/**\n * @property int $id\n * @mixin \\Eloquent\n */\nclass Post extends Model\n{\n}\n";
    let titles = rewrite_titles(&mut plugin, "/project/app/Models/Post.php", "php", text, "@property");
    assert!(titles.iter().any(|t| t.starts_with("Remove IDE helper PHPDoc")), "{titles:?}");
    let plain = "<?php\nclass Post extends Model\n{\n}\n";
    assert!(rewrite_titles(&mut plugin, "/project/app/Models/Post.php", "php", plain, "class").is_empty());
}

#[test]
fn livewire_view_calling_a_missing_method_is_reported() {
    let host = MockHost::install();
    host.add_file("app/Livewire/Tally.php", "<?php\nnamespace App\\Livewire;\nuse Livewire\\Component;\nclass Tally extends Component\n{\n    public function increment() {}\n}\n");
    host.add_file("resources/views/livewire/tally.blade.php", "<button wire:click=\"increment\">+</button>\n<button wire:click=\"decrement\">-</button>\n");
    let mut plugin = activated(&host);
    let path = "/project/resources/views/livewire/tally.blade.php";
    let text = host_text(&host, path);
    let project = host.invocation().project().unwrap();
    plugin.check(&project, path);
    let found = host.diagnostics();
    let codes: Vec<_> = found[path].iter().map(|d| (d.code.clone().unwrap(), d.range.start.line)).collect();
    assert_eq!(codes, [("laravel.unknown-livewire-method".to_owned(), 1)]);
    assert!(text.contains("decrement"));
}

#[test]
fn livewire_view_of_a_component_with_a_trait_stays_silent() {
    let host = MockHost::install();
    host.add_file("app/Livewire/Tally.php", "<?php\nclass Tally extends Component\n{\n    use Helpers;\n}\n");
    host.add_file("resources/views/livewire/tally.blade.php", "<button wire:click=\"anything\">+</button>\n");
    let mut plugin = activated(&host);
    let path = "/project/resources/views/livewire/tally.blade.php";
    let project = host.invocation().project().unwrap();
    plugin.check(&project, path);
    assert!(host.diagnostics().get(path).is_none_or(|d| d.is_empty()));
}

#[test]
fn routes_naming_missing_controllers_and_methods_are_reported() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    host.add_file(
        "routes/extra.php",
        "<?php\nuse App\\Http\\Controllers\\PostController;\nuse App\\Http\\Controllers\\GhostController;\nuse Vendor\\Pkg\\RemoteController;\nRoute::get('/a', [PostController::class, 'index']);\nRoute::get('/b', [PostController::class, 'nope']);\nRoute::get('/c', [GhostController::class, 'index']);\nRoute::get('/d', [RemoteController::class, 'index']);\nRoute::resource('posts', PostController::class);\n",
    );
    let path = "/project/routes/extra.php";
    let project = host.invocation().project().unwrap();
    plugin.index_project(&project);crate::tests::finish_indexing(&mut plugin);
    plugin.check(&project, path);
    let found: Vec<_> = host.diagnostics()[path].iter().map(|d| (d.code.clone().unwrap(), d.range.start.line)).collect();
    assert_eq!(
        found,
        [
            ("laravel.unknown-route-method".to_owned(), 5),
            ("laravel.unknown-route-controller".to_owned(), 6),
            ("laravel.missing-resource-methods".to_owned(), 8),
        ]
    );
}

#[test]
fn route_files_with_controller_groups_and_invokables_stay_silent() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    host.add_file("routes/grouped.php", "<?php\nRoute::controller(PostController::class)->group(function () {\n    Route::get('/x', 'missing');\n});\n");
    host.add_file("app/Http/Controllers/InvokableController.php", "<?php\nnamespace App\\Http\\Controllers;\nclass InvokableController { public function __invoke() {} }\n");
    host.add_file("routes/invoke.php", "<?php\nuse App\\Http\\Controllers\\InvokableController;\nRoute::get('/x', InvokableController::class);\n");
    let project = host.invocation().project().unwrap();
    plugin.index_project(&project);crate::tests::finish_indexing(&mut plugin);
    for path in ["/project/routes/grouped.php", "/project/routes/invoke.php"] {
        plugin.check(&project, path);
        assert!(host.diagnostics().get(path).is_none_or(|d| d.is_empty()), "{path}");
    }
}

#[test]
fn related_model_files_use_provider_navigation_without_rename_anchors() {
    use openidea_plugin::indexing::{Document as IndexDocument, DocumentIndexer};
    let host = MockHost::install();
    host.add_file("database/factories/PostFactory.php", "<?php\nclass PostFactory extends Factory {}\n");
    let mut plugin = activated(&host);
    let text = host_text(&host, "/project/app/Models/Post.php");
    let edges = plugin.index(&IndexDocument { path: "/project/app/Models/Post.php".into(), text: text.clone(), symbols: Vec::new() });
    assert!(edges.definitions.is_empty() && edges.references.is_empty() && edges.markers.is_empty());
    let markers = plugin.line_markers(&doc("/project/app/Models/Post.php", "php", &text));
    assert!(markers.iter().any(|m| m.target.as_ref().is_some_and(|l| l.path.ends_with("PostFactory.php"))), "{markers:?}");
    assert!(markers.iter().any(|m| m.target.as_ref().is_some_and(|l| l.path.ends_with("PostPolicy.php"))), "{markers:?}");
    assert!(!markers.iter().any(|m| m.target.as_ref().is_some_and(|l| l.path.ends_with("PostObserver.php"))));
}

fn selection_actions(plugin: &mut Main, path: &str, language: &str, text: &str, selected: &str) -> Vec<CodeAction> {
    let start = text.find(selected).unwrap();
    let range = Range::new(text::position(text, start), text::position(text, start + selected.len()));
    plugin.code_actions(&doc(path, language, text), range)
}

#[test]
fn selected_blade_text_is_extracted_to_the_main_locale_lang_file() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<h1>Hello there</h1>\n";
    let actions = selection_actions(&mut plugin, "/project/resources/views/x.blade.php", "blade", text, "Hello there");
    let action = actions.iter().find(|a| a.title.starts_with("Extract to translation key")).unwrap();
    assert!(action.title.contains("messages.hello_there"));
    assert_eq!(action.edits[0].1[0].text, "{{ __('messages.hello_there') }}");
    assert_eq!(action.edits[1].0, "/project/lang/en/messages.php");
    assert_eq!(action.edits[1].1[0].text, "    'hello_there' => 'Hello there',\n");

    let in_tag = "<a title=\"Hello there\">x</a>\n";
    let none = selection_actions(&mut plugin, "/project/resources/views/x.blade.php", "blade", in_tag, "Hello there");
    assert!(none.iter().all(|a| !a.title.starts_with("Extract")));
}

#[test]
fn selected_php_string_is_extracted_only_inside_its_quotes() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\nreturn 'Saved it';\n";
    let actions = selection_actions(&mut plugin, "/project/app/A.php", "php", text, "Saved it");
    assert_eq!(actions.iter().find(|a| a.title.starts_with("Extract")).unwrap().edits[0].1[0].text, "__('messages.saved_it')");
    let partial = selection_actions(&mut plugin, "/project/app/A.php", "php", text, "Saved");
    assert!(partial.iter().all(|a| !a.title.starts_with("Extract")));
}

#[test]
fn form_request_gets_rules_for_the_model_columns() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let text = "<?php\nclass StorePostRequest extends FormRequest\n{\n    public function rules(): array\n    {\n        return [\n            'title' => ['required'],\n        ];\n    }\n}\n";
    let at = text.find("return").unwrap();
    let position = text::position(text, at);
    let actions = plugin.code_actions(&doc("/project/app/Http/Requests/StorePostRequest.php", "php", text), Range::new(position, position));
    let action = actions.iter().find(|a| a.title.starts_with("Add validation rules")).unwrap();
    let inserted = &action.edits[0].1[0].text;
    assert!(inserted.contains("'body' => ['required', 'string']"), "{inserted}");
    assert!(!inserted.contains("'title'") && !inserted.contains("created_at"), "{inserted}");
}

#[test]
fn model_fillable_gets_the_missing_columns() {
    let host = MockHost::install();
    host.add_file("app/Models/Post.php", "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nclass Post extends Model\n{\n    protected $fillable = [\n        'title',\n    ];\n}\n");
    let mut plugin = Main::default();
    plugin.activate(&host.context("aritti.laravel")).unwrap();crate::tests::finish_indexing(&mut plugin);
    laravel_project_without_post(&host);
    plugin.index_all();crate::tests::finish_indexing(&mut plugin);
    let path = "/project/app/Models/Post.php";
    let text = host_text(&host, path);
    let position = text::position(&text, text.find("$fillable").unwrap());
    let actions = plugin.code_actions(&doc(path, "php", &text), Range::new(position, position));
    let action = actions.iter().find(|a| a.title == "Add the missing columns to $fillable").unwrap();
    assert_eq!(action.edits[0].1[0].text, "\n        'body',");
}

fn laravel_project_without_post(host: &MockHost) {
    host.add_file("composer.json", r#"{"require": {"laravel/framework": "^11.0"}}"#);
    host.add_file(
        "database/migrations/2024_01_01_create_posts_table.php",
        "<?php\nreturn new class extends Migration {\n    public function up(): void\n    {\n        Schema::create('posts', function (Blueprint $table) {\n            $table->id();\n            $table->string('title');\n            $table->text('body');\n            $table->timestamps();\n        });\n    }\n};\n",
    );
}

#[test]
fn component_attributes_and_slots_complete_from_props_and_the_view() {
    let host = MockHost::install();
    host.add_file("resources/views/components/banner.blade.php", "@props(['tone' => 'info', 'dismissible'])\n<div>{{ $slot }}{{ $footer }}</div>\n");
    let mut plugin = activated(&host);
    let path = "/project/resources/views/x.blade.php";

    let (text, at) = cursor("<x-banner ton|");
    assert_eq!(labels(plugin.completion(&doc(path, "blade", &text), at, None)), ["tone", "dismissible"]);

    let (text, at) = cursor("<x-banner>\n<x-slot:fo|");
    assert_eq!(labels(plugin.completion(&doc(path, "blade", &text), at, None)), ["footer"]);

    let (text, at) = cursor("<x-alert |");
    assert!(labels(plugin.completion(&doc(path, "blade", &text), at, None)).is_empty());
}

#[test]
fn livewire_tag_attributes_complete_from_public_properties() {
    let host = MockHost::install();
    host.add_file("app/Livewire/Tally.php", "<?php\nclass Tally extends Component\n{\n    public int $count = 0;\n}\n");
    let mut plugin = activated(&host);
    let (text, at) = cursor("<livewire:tally |");
    assert_eq!(labels(plugin.completion(&doc("/project/resources/views/x.blade.php", "blade", &text), at, None)), ["count"]);
}

#[test]
fn columns_complete_through_relations_and_scopes() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    host.add_file("app/Models/Thread.php", "<?php\nclass Thread extends Model\n{\n    public function replies(): HasMany\n    {\n        return $this->hasMany(Reply::class);\n    }\n\n    public function scopeActive($query) {}\n}\n");
    host.add_file("app/Models/Reply.php", "<?php\nclass Reply extends Model {}\n");
    host.add_file(
        "database/migrations/2024_02_01_create_replies_table.php",
        "<?php\nreturn new class extends Migration {\n    public function up(): void\n    {\n        Schema::create('replies', function (Blueprint $table) {\n            $table->id();\n            $table->string('author');\n        });\n    }\n};\n",
    );
    plugin.index_all();crate::tests::finish_indexing(&mut plugin);
    let path = "/project/app/Http/Controllers/A.php";

    for marked in [
        "<?php\nThread::find(1)->replies()->where('au|');",
        "<?php\nThread::active()->replies()->orderBy('au|');",
        "<?php\nThread::query()->with('x')->replies()->latest()->where('au|');",
    ] {
        let (text, at) = cursor(marked);
        assert_eq!(labels(plugin.completion(&doc(path, "php", &text), at, None)), ["author", "id"], "{marked}");
    }

    let (text, at) = cursor("<?php\nThread::find(1)->unknown()->where('au|');");
    assert!(labels(plugin.completion(&doc(path, "php", &text), at, None)).is_empty());

    let (text, at) = cursor("<?php\nPost::where('ti|');");
    assert!(labels(plugin.completion(&doc(path, "php", &text), at, None)).contains(&"title".to_owned()));

    let (text, at) = cursor("<?php\nclass Thread extends Model\n{\n    function a() { return $this->replies()->where('au|'); }\n}\n");
    assert_eq!(labels(plugin.completion(&doc("/project/app/Models/Thread.php", "php", &text), at, None)), ["author", "id"]);
}

#[test]
fn container_names_complete_and_navigate() {
    let host = MockHost::install();
    host.add_file("app/Services/Billing.php", "<?php\nnamespace App\\Services;\n\nclass Billing\n{\n}\n");
    host.add_file("app/Providers/AppServiceProvider.php", "<?php\nnamespace App\\Providers;\n\nclass AppServiceProvider\n{\n    public function register()\n    {\n        $this->app->singleton('billing.gateway', fn () => 1);\n    }\n}\n");
    let mut plugin = activated(&host);
    let path = "/project/app/Http/Controllers/A.php";

    let (text, at) = cursor("<?php\napp('ca|');");
    let names = labels(plugin.completion(&doc(path, "php", &text), at, None));
    for expected in ["cache", "billing.gateway", "App\\Services\\Billing"] {
        assert!(names.contains(&expected.to_owned()), "{expected} in {names:?}");
    }

    let (text, at) = cursor("<?php\nApp::make('App\\\\Services\\\\Bill|ing');");
    let found = plugin.definition(&doc(path, "php", &text), at);
    assert_eq!(found.len(), 1);
    assert!(found[0].path.ends_with("app/Services/Billing.php"));

    let (text, at) = cursor("@inject('b', 'App\\Services\\Bil|ling')");
    assert_eq!(plugin.definition(&doc("/project/resources/views/x.blade.php", "blade", &text), at).len(), 1);
}

#[test]
fn config_app_providers_complete_project_providers() {
    let host = MockHost::install();
    host.add_file("app/Providers/BillingServiceProvider.php", "<?php\nnamespace App\\Providers;\n\nclass BillingServiceProvider\n{\n}\n");
    let mut plugin = activated(&host);
    let path = "/project/config/app.php";

    let (text, at) = cursor("<?php\nreturn [\n    'name' => 'x',\n    'providers' => [\n        App\\Providers\\Bil|\n    ],\n];\n");
    let items = plugin.completion(&doc(path, "php", &text), at, None);
    let item = items.iter().find(|i| i.label == "App\\Providers\\BillingServiceProvider").unwrap();
    assert_eq!(item.insert_text, "App\\Providers\\BillingServiceProvider::class");

    let (text, at) = cursor("<?php\nreturn [\n    'name' => 'Bil|',\n    'providers' => [\n    ],\n];\n");
    assert!(plugin.completion(&doc(path, "php", &text), at, None).iter().all(|i| i.label != "App\\Providers\\BillingServiceProvider"));

    let (text, at) = cursor("<?php\nreturn [\n    'providers' => [\n        App\\Providers\\Billing|ServiceProvider::class,\n    ],\n];\n");
    assert_eq!(plugin.definition(&doc(path, "php", &text), at).len(), 1);
}

#[test]
fn generate_for_a_selected_model_prepares_an_atomic_factory_creation() {
    let host = MockHost::install().with_capabilities(&["project.read", "project.write", "events", "process:php", "editor.read"]);
    let mut plugin = activated(&host);
    host.select_paths(&["/project/app/Models/Post.php"]);
    host.answer_next_prompt(Some(&["factory"]));
    host.answer_next_prompt(Some(&["PostFactory"]));
    host.answer_next_prompt(Some(&["all"]));
    plugin.run_command("generate-for-model", &host.invocation(), None).unwrap();

    let project=host.invocation().project().unwrap();
    assert!(!project.exists("database/factories/PostFactory.php").unwrap());
    use openidea_plugin::file_actions::{FileActions,FileOperation};
    let source=project.read_file("app/Models/Post.php").unwrap();let document=doc("/project/app/Models/Post.php","php",&source);
    let action=plugin.file_actions(&document,Range::new(Position::new(0,0),Position::new(0,0))).into_iter().find(|action|action.data.as_deref().is_some_and(|key|key.starts_with("bulk-generate:"))).unwrap();
    host.answer_next_prompt(Some(&["yes"]));let action=plugin.resolve_file_action(&document,action).unwrap();
    let [FileOperation::Create{path,text:body,overwrite}]=action.operations.as_slice() else{panic!("one atomic creation")};
    assert!(path.ends_with("database/factories/PostFactory.php"));assert!(!overwrite);
    assert!(!project.exists(path).unwrap(),"preparing returns an edit without writing");
    assert!(host.process_runs().is_empty());
    assert!(body.contains("'title' => fake()->sentence(),") && body.contains("'body' => fake()->paragraph(),"), "{body}");
    assert!(!body.contains("created_at"), "{body}");
    assert!(host.terminals().is_empty());
}

#[test]
fn resource_body_is_written_after_confirming_without_running_artisan() {
    let host = MockHost::install().with_capabilities(&["project.read", "project.write", "events", "process:php", "editor.read"]);
    let mut plugin = activated(&host);
    host.select_paths(&["/project/app/Models/Post.php"]);
    host.answer_next_prompt(Some(&["resource"]));
    host.answer_next_prompt(Some(&["PostResource"]));
    host.answer_next_prompt(Some(&["all"]));
    host.answer_next_prompt(Some(&[]));
    plugin.run_command("generate-for-model", &host.invocation(), None).unwrap();
    assert!(host.terminals().is_empty());
    assert!(host.process_runs().is_empty());
    let written = host.invocation().project().unwrap().read_file("app/Http/Resources/PostResource.php").unwrap();
    assert!(written.contains("'title' => $this->title,"), "{written}");

    let index = plugin.indexes.values().next().unwrap();
    let source = bulk::resource_source(index, "Post", "PostResource");
    assert!(source.contains("'title' => $this->title,") && source.contains("'created_at' => $this->created_at,"), "{source}");
}

#[test]
fn remove_phpdocs_rewrites_every_model_that_has_helper_tags() {
    let host = MockHost::install().with_capabilities(&["project.read", "project.write", "events", "process:php", "editor.read"]);
    host.add_file("app/Models/Tag.php", "<?php\n/**\n * @property int $id\n */\nclass Tag extends Model {}\n");
    let mut plugin = activated(&host);
    use openidea_plugin::file_actions::FileActions;
    let text = host.invocation().project().unwrap().read_file("app/Models/Tag.php").unwrap();
    let doc = openidea_plugin::providers::Document { path:"/project/app/Models/Tag.php".into(), language:"php".into(), text, version:1 };
    let action = plugin.file_actions(&doc,Range::new(Position::new(0,0),Position::new(0,0))).into_iter().find(|a|a.data.as_deref().is_some_and(|d|d.starts_with("bulk-phpdocs:"))).unwrap();
    host.answer_next_prompt(Some(&[]));
    let resolved = plugin.resolve_file_action(&doc,action).unwrap();
    assert_eq!(resolved.edits.len(),1);
    assert_eq!(resolved.edits[0].0,"/project/app/Models/Tag.php");
    assert_eq!(resolved.edits[0].1[0].text,"<?php\nclass Tag extends Model {}\n");
    assert!(host.process_runs().is_empty());
}

#[test]
fn new_generators_run_their_own_artisan_commands_when_the_package_is_installed() {
    let host = MockHost::install().with_capabilities(&["project.read", "events", "process:php", "editor.read"]);
    host.add_file("composer.json", r#"{"require": {"laravel/framework": "^11.0", "laravel/nova": "^5.0"}}"#);
    let mut plugin = activated(&host);
    host.add_file("composer.json", r#"{"require": {"laravel/framework": "^11.0", "laravel/nova": "^5.0"}}"#);
    host.answer_next_prompt(Some(&["nova:resource"]));
    host.answer_next_prompt(Some(&["Post"]));
    plugin.run_command("make", &host.invocation(), None).unwrap();
    assert_eq!(terminal_args(&host, &plugin), [["artisan", "nova:resource", "Post"]]);
}

#[test]
fn indexing_sets_the_menu_context_and_reveal_brings_the_tool_window_forward() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    assert!(host.context_keys().contains_key("indexed"));
    plugin.run_command("reveal", &host.invocation(), None).unwrap();
    assert_eq!(host.revealed_views(), ["laravel"]);
}

#[test]
fn extra_model_directories_and_the_resource_check_setting_are_honoured() {
    let host = MockHost::install();
    laravel_project(&host);
    host.add_file("modules/Blog/Models/Comment.php", "<?php\nclass Comment extends Model {}\n");
    host.set_setting("modelDirectories", "\"modules/Blog/Models, ../outside\"");
    let mut plugin = Main::default();
    plugin.activate(&host.context("aritti.laravel")).unwrap();crate::tests::finish_indexing(&mut plugin);
    let index = plugin.indexes.values().next().unwrap();
    assert!(index.models.contains_key("Comment"));

    host.add_file("routes/res.php", "<?php\nuse App\\Http\\Controllers\\PostController;\nRoute::resource('p', PostController::class);\n");
    host.set_setting("reportMissingResourceMethods", "false");
    let project = host.invocation().project().unwrap();
    plugin.index_project(&project);crate::tests::finish_indexing(&mut plugin);
    plugin.check(&project, "/project/routes/res.php");
    assert!(host.diagnostics().get("/project/routes/res.php").is_none_or(|d| d.is_empty()));
}

#[test]
fn create_missing_makes_the_class_named_by_new_and_refuses_existing_ones() {
    let host = MockHost::install().with_capabilities(&["project.read", "events", "process:php", "editor.read", "editor.write"]);
    host.add_file("app/Jobs/ExistingJob.php", "<?php\nclass ExistingJob {}\n");
    let mut plugin = activated(&host);
    let text = "<?php\ndispatch(new SendReportJob());\ndispatch(new ExistingJob());\n";
    host.open_editor("/project/app/Http/X.php", "php", text, &[(1, 18, 1, 18)]);
    host.answer_next_prompt(Some(&[]));
    plugin.run_command("create-missing", &host.invocation(), None).unwrap();
    assert_eq!(terminal_args(&host, &plugin), [["artisan", "make:job", "SendReportJob"]]);

    host.open_editor("/project/app/Http/X.php", "php", text, &[(2, 18, 2, 18)]);
    assert!(plugin.run_command("create-missing", &host.invocation(), None).is_err());
}

fn route_rows(view: &View) -> Vec<(String, String, String)> {
    let Body::List(items) = &view.body else { panic!("not a list") };
    items
        .iter()
        .map(|item| (item.key.clone(), item.label.clone(), item.detail.clone().unwrap_or_default()))
        .collect()
}

#[test]
fn laravel_window_has_separate_resource_tabs() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let ctx = host.invocation();
    let view = plugin.render(VIEW, None, &ctx).unwrap();
    assert!(matches!(view.body, Body::List(_)));
    assert!(view.toolbar.iter().all(|b| !b.id.starts_with("section:")));
    let view = plugin.render("laravel-middleware", None, &ctx).unwrap();
    let Body::Tree(items) = view.body else { panic!("not a resource list") };
    assert!(items.iter().all(|item| item.key.starts_with("middleware:") && item.parent.is_none()));
    assert!(view.toolbar.iter().all(|b| !b.id.starts_with("section:")));
    let view = plugin.render("laravel-env", None, &ctx).unwrap();
    let Body::Other { kind, data } = view.body else { panic!("not a resource table") };
    assert_eq!(kind, "resources");
    let table: serde_json::Value = serde_json::from_str(&data).unwrap();
    assert_eq!(table["columns"], serde_json::json!(["Name", "Value", "File"]));
    assert!(table["items"].as_array().unwrap().iter().all(|item| item["key"].as_str().unwrap().starts_with("env:")));
    assert!(view.toolbar.iter().all(|b| !b.id.starts_with("section:")));
    let group: serde_json::Value = serde_json::from_str(include_str!("ui_group.json")).unwrap();
    let tabs = group["tabs"].as_array().unwrap();
    assert_eq!(tabs.len(), 15);
    assert!(tabs.iter().all(|tab| tab["icon"].as_str().is_some_and(|icon| !icon.is_empty())));
}

#[test]
fn routes_show_names_and_controller_methods() {
    let host = MockHost::install();
    let mut plugin = activated(&host);

    let view = plugin.render(VIEW, None, &host.invocation()).unwrap();
    let rows = route_rows(&view);
    assert_eq!(rows[1], ("route:1".into(), "/posts".into(), "[GET] PostController@index \u{b7} posts.index".into()));
    assert_eq!(rows[3], ("route:3".into(), "/admin/dashboard".into(), "[GET] PostController@index \u{b7} admin.dashboard".into()));
}

#[test]
fn closure_routes_list_under_closure() {
    let host = MockHost::install();
    let mut plugin = activated(&host);

    let view = plugin.render(VIEW, None, &host.invocation()).unwrap();
    assert_eq!(route_rows(&view)[0], ("route:0".into(), "/".into(), "[GET] Closure \u{b7} home".into()));
}

#[test]
fn filter_by_controller() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let ctx = host.invocation();

    plugin.render(VIEW, None, &ctx).unwrap();
    host.answer_next_prompt(Some(&["Closure"]));
    plugin.handle(VIEW, ViewEvent::Button("routes:filter".into()), &ctx).unwrap();
    let view = plugin.render(VIEW, None, &ctx).unwrap();
    assert_eq!(route_rows(&view).len(), 1);
    assert_eq!(view.status.as_deref(), Some("1 endpoints \u{b7} controller: Closure · Routes read from source files · Refresh Registered Routes to include package routes"));
}

#[test]
fn activating_a_route_row_opens_its_definition() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let ctx = host.invocation();

    plugin.handle(VIEW, ViewEvent::Activate("route:0".into()), &ctx).unwrap();
    assert_eq!(host.opened_locations().len(), 1);
}

#[test]
fn curl_row_action_copies_the_route() {
    let host = MockHost::install();
    let mut plugin = activated(&host);

    host.answer_next_prompt(Some(&["http://localhost:8000"]));
    plugin.run_command("route-copy-curl", &host.invocation(), Some("route:1")).unwrap();
    assert_eq!(host.clipboard().as_deref(), Some("curl -X GET 'http://localhost:8000/posts'"));
}

#[test]
fn factory_navigation_does_not_rename_the_model_or_factory_declarations() {
    use openidea_plugin::indexing::{Document as IndexDocument,DocumentIndexer};
    let host = MockHost::install();
    host.add_file("database/factories/PostFactory.php","<?php class PostFactory extends Factory {}");
    let mut plugin = activated(&host);
    let usage = "<?php class FactoryUsage { function run() { Post::factory(); } }";
    let current = doc("/project/app/Http/Controllers/FactoryUsage.php","php",usage);
    let target = plugin.definition(&current, text::position(usage,usage.find("Post::").unwrap()+2));
    assert!(target.iter().any(|t| t.path.ends_with("PostFactory.php")), "{target:?}");
    let exported = plugin.index(&IndexDocument { path:current.path.clone(),text:usage.into(),symbols:Vec::new() });
    assert!(exported.definitions.is_empty() && exported.references.is_empty());
    host.add_file("database/factories/PostFactory.php","");
    plugin.handle_events(&[Event::FilesChanged(openidea_plugin::FilesChanged { removed:vec!["/project/database/factories/PostFactory.php".into()], ..Default::default() })],&host.invocation());crate::tests::finish_indexing(&mut plugin);
    assert!(plugin.definition(&current,text::position(usage,usage.find("Post::").unwrap()+2)).is_empty());
}

#[test]
fn unsaved_related_target_changes_refresh_ranges_and_remove_stale_links() {
    let host = MockHost::install();
    host.add_file("database/factories/PostFactory.php","<?php class PostFactory extends Factory {}");
    let mut plugin = activated(&host);
    let usage = "<?php class FactoryUsage { function run() { Post::factory(); } }";
    let source = doc("/project/app/Http/Controllers/FactoryUsage.php","php",usage);
    let at = text::position(usage,usage.find("Post::").unwrap()+1);
    assert!(!plugin.definition(&source,at).is_empty());
    host.open_editor("/project/database/factories/PostFactory.php","php","<?php\n\nclass PostFactory extends Factory {}",&[]);
    assert_eq!(plugin.definition(&source,at)[0].range.start.line,2);
    host.open_editor("/project/database/factories/PostFactory.php","php","<?php class RenamedFactory extends Factory {}",&[]);
    assert!(plugin.definition(&source,at).is_empty());
}

#[test]
fn watched_listener_and_observer_changes_refresh_related_navigation() {
    let host = MockHost::install();
    host.add_file("app/Observers/PostObserver.php","<?php class PostObserver {}");
    host.add_file("app/Events/Ready.php","<?php class Ready {}");
    host.add_file("app/Listeners/ListenReady.php","<?php class ListenReady { function handle(Ready $event) {} }");
    let mut plugin = activated(&host);
    let event = doc("/project/app/Events/Ready.php","php","<?php class Ready {}");
    assert!(plugin.line_markers(&event).iter().any(|m|m.target.as_ref().is_some_and(|l|l.path.ends_with("ListenReady.php"))));
    host.add_file("app/Listeners/ListenReady.php","<?php class ListenReady { function handle(Other $event) {} }");
    plugin.handle_events(&[Event::FilesChanged(openidea_plugin::FilesChanged { modified:vec![event.path.replace("Events/Ready","Listeners/ListenReady")], ..Default::default() })],&host.invocation());crate::tests::finish_indexing(&mut plugin);
    assert!(!plugin.line_markers(&event).iter().any(|m|m.target.as_ref().is_some_and(|l|l.path.ends_with("ListenReady.php"))));
    host.add_file("app/Listeners/ListenReady.php","<?php class ListenReady { function handle(Ready $event) {} }");
    plugin.handle_events(&[Event::FilesChanged(openidea_plugin::FilesChanged { modified:vec!["/project/app/Listeners/ListenReady.php".into()], ..Default::default() })],&host.invocation());crate::tests::finish_indexing(&mut plugin);
    assert!(plugin.line_markers(&event).iter().any(|m|m.target.as_ref().is_some_and(|l|l.path.ends_with("ListenReady.php"))));
    for relative in ["app/Events/Ready.php","app/Observers/PostObserver.php","database/seeders/PostSeeder.php","app/Post.php"] { assert!(affects_index(relative),"{relative}"); }
}


fn packaged_invalidations(host: &MockHost) -> usize {
    host.view_messages().iter().filter(|(view,raw)|view==VIEW && raw==r#"{"channel":"ui-kit-refresh"}"#).count()
}
#[test]
fn observed_route_reindex_refreshes_mounted_cache_without_render_cli_or_loop() {
    let host=MockHost::install();let mut plugin=activated(&host);
    assert_eq!(packaged_invalidations(&host),1);
    host.on_process("php",|_,_|panic!("cached render/reindex must not invoke Artisan"));
    host.add_file("routes/web.php","<?php Route::get('/observed', fn()=>1)->name('observed');");
    plugin.handle_events(&[Event::FileSaved("/project/routes/web.php".into())],&host.invocation());crate::tests::finish_indexing(&mut plugin);
    assert_eq!(packaged_invalidations(&host),3);
    for _ in 0..3 {
        let view=plugin.render(VIEW,None,&host.invocation()).unwrap();
        assert!(route_rows(&view).iter().any(|(_,path,_)|path=="/observed"));
    }
    assert_eq!(packaged_invalidations(&host),3);assert!(host.process_runs().is_empty());assert!(host.requests().is_empty());
}
#[test]
fn loss_of_laravel_marker_retires_old_index_and_notifies_once() {
    let host=MockHost::install();let mut plugin=activated(&host);
    host.add_file("composer.json","{}");
    plugin.handle_events(&[Event::FilesChanged(openidea_plugin::FilesChanged {modified:vec!["/project/composer.json".into()],..Default::default()})],&host.invocation());crate::tests::finish_indexing(&mut plugin);
    assert!(!plugin.indexes.contains_key("/project"));assert_eq!(packaged_invalidations(&host),2);
    assert!(matches!(plugin.render(VIEW,None,&host.invocation()).unwrap().body,Body::Message(_)));
    plugin.index_project(&host.invocation().project().unwrap());crate::tests::finish_indexing(&mut plugin);
    assert_eq!(packaged_invalidations(&host),2,"no existing cache, no further removal notification");
}
#[test]
fn missing_foreign_or_denied_owner_render_withholds_but_preserves_another_root_cache() {
    for case in 0..3 {
        let host=MockHost::install();let mut plugin=activated(&host);
        let host=match case {
            0=>{host.add_project("/project");host.set_project_root(Some("/foreign"));host},
            1=>{host.set_project_root(None);host},
            _=>host.with_capabilities(&[]),
        };
        assert!(matches!(plugin.render(VIEW,None,&host.invocation()).unwrap().body,Body::Message(_)));
        assert!(plugin.indexes.contains_key("/project"));assert_eq!(packaged_invalidations(&host),1);
        assert!(host.opened_locations().is_empty());
    }
}
#[test]
fn exact_selected_root_has_no_implicit_nested_app_fallback() {
    let host=MockHost::install();let mut plugin=activated(&host);
    host.add_project("/project");host.set_project_root(Some("/project/nested"));laravel_project(&host);
    host.add_file("routes/web.php","<?php Route::get('/nested-only', fn()=>1)->name('nested');");
    plugin.index_project(&host.invocation().project().unwrap());crate::tests::finish_indexing(&mut plugin);
    assert!(route_rows(&plugin.render(VIEW,None,&host.invocation()).unwrap()).iter().any(|(_,path,_)|path=="/nested-only"));
    host.add_project("/project/nested");host.set_project_root(Some("/project"));
    let outer=plugin.render(VIEW,None,&host.invocation()).unwrap();assert!(!route_rows(&outer).iter().any(|(_,path,_)|path=="/nested-only"));
    host.add_file("composer.json","{}");plugin.index_project(&host.invocation().project().unwrap());crate::tests::finish_indexing(&mut plugin);
    assert!(matches!(plugin.render(VIEW,None,&host.invocation()).unwrap().body,Body::Message(_)));
    assert!(plugin.indexes.contains_key("/project/nested"),"outer render/removal must preserve explicit nested app cache");
}
#[test]
fn direct_old_route_event_after_source_reindex_cannot_open_removed_route() {
    let host=MockHost::install();let mut plugin=activated(&host);
    plugin.render(VIEW,None,&host.invocation()).unwrap();
    host.add_file("routes/web.php","<?php // all routes removed");
    plugin.handle_events(&[Event::FileSaved("/project/routes/web.php".into())],&host.invocation());crate::tests::finish_indexing(&mut plugin);
    assert!(plugin.handle(VIEW,ViewEvent::Activate("route:0".into()),&host.invocation()).is_err());
    assert!(host.opened_locations().is_empty());
}
#[test]
fn explicit_reindex_command_updates_hub_and_keeps_source_native_endpoint_exports() {
    use openidea_plugin::endpoints::EndpointsProvider;
    let host=MockHost::install();let mut plugin=activated(&host);
    host.add_file("routes/web.php","<?php Route::post('/new-endpoint', fn()=>1)->name('new');");
    plugin.run_command("reindex",&host.invocation(),None).unwrap();crate::tests::finish_indexing(&mut plugin);
    assert_eq!(packaged_invalidations(&host),3);
    let endpoints=plugin.endpoints(&host.invocation()).unwrap();
    assert!(endpoints.iter().any(|endpoint|endpoint.path=="/new-endpoint"));
    assert!(host.process_runs().is_empty());assert!(host.requests().is_empty());
}

#[test]
fn framework_navigation_selects_exact_utf16_source_tokens_without_composed_rename_anchors() {
    use openidea_plugin::indexing::{Document as IndexDocument, DocumentIndexer};
    let host = MockHost::install();
    laravel_project(&host);
    host.add_file("routes/web.php", "<?php\r\n// 😀\r\nRoute::name('admin.')->group(function () { Route::get('/x', 'App\\Http\\Controllers\\PostController@index')->name('é😀'); });\r\n");
    host.add_file("config/app.php", "<?php\r\nreturn ['parent' => ['é😀' => 'value']];\r\n");
    host.add_file("lang/en/messages.php", "<?php\r\nreturn ['é😀' => 'Hello'];\r\n");
    host.add_file("lang/en.json", "{\r\n  \"say \\\"hi\\\" 😀\": \"Hi\"\r\n}\r\n");
    let mut plugin = Main::default();
    plugin.activate(&host.context("aritti.laravel")).unwrap();crate::tests::finish_indexing(&mut plugin);
    let selected = |target: &Location| {
        let source = host_text(&host, &target.path);
        let start = text::offset(&source, target.range.start).unwrap();
        let end = text::offset(&source, target.range.end).expect("navigation end must remain within UTF16 source");
        source[start..end].to_owned()
    };
    for (marked, target_path) in [
        ("<?php route('admin.é|😀');", "/project/routes/web.php"),
        ("<?php config('app.parent.é|😀');", "/project/config/app.php"),
        ("<?php __('messages.é|😀');", "/project/lang/en/messages.php"),
    ] {
        let (source, at) = cursor(marked);
        let targets = plugin.definition(&doc("/project/app/Test.php", "php", &source), at);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, target_path);
        assert_eq!(selected(&targets[0]), "é😀", "{marked}");
    }
    let index = plugin.index_for("/project/app/Test.php").unwrap();
    assert_eq!(selected(&route_list_of(index)[0].location.clone().unwrap()), "é😀");
    let (path, at, width) = entry_for(index, "route:0").unwrap();
    assert_eq!(selected(&Location { path: path.into(), range: Range::new(at, Position::new(at.line, at.character + width)) }), "é😀");
    let json = location(&index.translations["say \"hi\" 😀"]).unwrap();
    assert_eq!(selected(&json), "say \\\"hi\\\" 😀", "decoded JSON key must select its full raw spelling");
    let controller = host_text(&host, "app/Http/Controllers/PostController.php");
    let markers = plugin.line_markers(&doc("/project/app/Http/Controllers/PostController.php", "php", &controller));
    let route = markers.iter().find(|m| m.tooltip == "Route GET|HEAD /x").unwrap();
    assert_eq!(selected(route.target.as_ref().unwrap()), "é😀");
    for path in ["/project/routes/web.php", "/project/config/app.php", "/project/lang/en/messages.php", "/project/lang/en.json"] {
        let edges = plugin.index(&IndexDocument { path:path.into(), text:host_text(&host,path), symbols:Vec::new() });
        assert!(edges.definitions.is_empty() && edges.references.is_empty() && edges.markers.is_empty());
    }
}

#[test]
fn registered_column_completion_does_not_reuse_another_function_or_shadowed_variable_assignment() {
    let host=MockHost::install();let mut plugin=activated(&host);let path="/project/app/Http/Controllers/X.php";
    let observations:Vec<_>=[
        "<?php function first(){ $query = Post::query(); } function second(){ $query->where('|'); }",
        "<?php function first(){ $query = Post::query(); } function second($query){ $query->where('|'); }",
        "<?php $query = Post::query(); $callback = fn($query) => $query->where('|');",
        "<?php $query = Post::query(); [$query] = getUnknown(); $query->where('|');",
        "<?php $query = Post::query(); unset($query); $query->where('|');",
    ].into_iter().map(|marked|{let (text,at)=cursor(marked);(marked,labels(plugin.completion(&doc(path,"php",&text),at,None)))}).collect();
    assert!(observations.iter().all(|(_,items)|items.is_empty()),"{observations:?}");
}

#[test]
fn registered_providers_do_not_inherit_outer_cache_when_current_nested_owner_is_cold() {
    let host=MockHost::install();let mut plugin=activated(&host);host.add_project("/project/nested");
    let observed:Vec<_>=[("/project/nested/test.php","php","<?php view('posts.in|dex');"),("/project/nested/resources/views/test.blade.php","blade","{{ view('posts.in|dex') }}"),("/project/nested/main.ts","typescript","route('ho|me')")].into_iter().map(|(path,language,marked)|{let(text,at)=cursor(marked);let d=doc(path,language,&text);(language,plugin.completion(&d,at,None).len(),plugin.definition(&d,at).len())}).collect();
    assert!(observed.iter().all(|(_,completions,definitions)|*completions==0 && *definitions==0),"{observed:?}");
    assert!(plugin.indexes.contains_key("/project") && !plugin.indexes.contains_key("/project/nested"));assert!(host.process_runs().is_empty() && host.requests().is_empty());
}
#[test]
fn registered_provider_unknown_or_denied_owner_withholds_cached_php_and_script_inventory_without_clearing_it() {
    for denied in [false,true] {
        let host=MockHost::install();let mut plugin=activated(&host);let host=if denied{host.with_capabilities(&[])}else{host.set_project_root(None);host};
        let observed:Vec<_>=[("/project/test.php","php","<?php view('posts.in|dex');"),("/project/main.ts","typescript","route('ho|me')")].into_iter().map(|(path,language,marked)|{let(text,at)=cursor(marked);let d=doc(path,language,&text);(language,plugin.completion(&d,at,None).len(),plugin.definition(&d,at).len())}).collect();
        assert!(observed.iter().all(|(_,c,d)|*c==0 && *d==0),"denied={denied} {observed:?}");assert!(plugin.indexes.contains_key("/project"));assert!(host.process_runs().is_empty());
    }
}

#[test]
fn current_document_owner_selects_exact_warm_cache_across_projects_not_invoking_window() {
    let host=MockHost::install();let mut plugin=activated(&host);host.add_project("/project");host.set_project_root(Some("/project/nested"));laravel_project(&host);
    host.add_file("routes/web.php","<?php Route::get('/nested-only',fn()=>1)->name('nested-only');");plugin.index_project(&host.invocation().project().unwrap());crate::tests::finish_indexing(&mut plugin);
    for (path,present,absent) in [("/project/main.ts","home","nested-only"),("/project/nested/main.ts","nested-only","home")] {
        let(text,at)=cursor("route('ho|me')");let found=labels(plugin.completion(&doc(path,"typescript",&text),at,None));assert!(found.iter().any(|v|v==present) && !found.iter().any(|v|v==absent),"{path}: {found:?}");
    }
    assert!(plugin.indexes.contains_key("/project") && plugin.indexes.contains_key("/project/nested"));assert!(host.requests().is_empty() && host.process_runs().is_empty());
}

#[test]
fn registered_blade_usage_markers_do_not_index_raw_php_string_directives() {
    let host=MockHost::install();laravel_project(&host);host.add_file("resources/views/fake-source.blade.php",r#"<?php $x="?> @include('posts.index')"; ?>"#);
    let mut plugin=Main::default();plugin.activate(&host.context("aritti.laravel")).unwrap();crate::tests::finish_indexing(&mut plugin);
    let d=doc("/project/resources/views/posts/index.blade.php","blade","<div>Post</div>");let found=plugin.line_markers(&d);
    assert!(!found.iter().any(|m|m.tooltip.starts_with("Used by") && m.tooltip.contains("fake-source")),"{found:?}");
}

#[test]
fn registered_layout_definition_and_marker_ranges_select_unicode_source_token_only() {
    let host=MockHost::install();laravel_project(&host);let layout="😀\r\n@yield('hé😀')\r\n";host.add_file("resources/views/layouts/unicode.blade.php",layout);
    let mut plugin=Main::default();plugin.activate(&host.context("aritti.laravel")).unwrap();crate::tests::finish_indexing(&mut plugin);let (source,at)=cursor("@extends('layouts.unicode')\n@section('hé|😀')");let d=doc("/project/resources/views/new.blade.php","blade",&source);
    let definition=plugin.definition(&d,at).remove(0);let marker=plugin.line_markers(&d).into_iter().find(|m|m.tooltip.starts_with("Parent section")).unwrap().target.unwrap();let start=layout.find("hé😀").unwrap();let expected=range_of(layout,start,start+"hé😀".len());
    assert_eq!((definition.range,marker.range),(expected,expected));
}
#[test]
fn registered_layout_requests_with_stale_short_or_mid_utf8_target_offsets_do_not_panic_or_emit_old_names() {
    let mut observed=Vec::new();
    for mid_utf8 in [false,true] {
        let host=MockHost::install();let mut plugin=activated(&host);let old=host_text(&host,"resources/views/layouts/app.blade.php");let offset=old.find("content").unwrap();let edited=if mid_utf8{format!("{}😀"," ".repeat(offset-1))}else{"x".into()};host.open_editor("/project/resources/views/layouts/app.blade.php","blade",&edited,&[]);
        let (source,at)=cursor("@extends('layouts.app')\n@section('co|ntent')");let d=doc("/project/resources/views/new.blade.php","blade",&source);
        let completion=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||plugin.completion(&d,at,None)));
        let markers=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||plugin.line_markers(&d)));
        observed.push((mid_utf8,completion.as_ref().map(|v|v.len()).ok(),markers.as_ref().map(|v|v.iter().filter(|m|m.tooltip.starts_with("Parent section")).count()).ok()));
    }
    assert!(observed.iter().all(|(_,c,m)|*c==Some(0) && *m==Some(0)),"{observed:?}");
}
#[test]
fn registered_layout_locations_follow_current_unsaved_declarations_not_old_offsets() {
    let host=MockHost::install();let mut plugin=activated(&host);let edited=format!("{}\r\n😀 @yield('content')\r\n@stack('scripts')"," ".repeat(120));host.open_editor("/project/resources/views/layouts/app.blade.php","blade",&edited,&[]);
    let (source,at)=cursor("@extends('layouts.app')\n@section('con|tent')");let d=doc("/project/resources/views/new.blade.php","blade",&source);let definition=plugin.definition(&d,at).remove(0);let marker=plugin.line_markers(&d).into_iter().find(|m|m.tooltip.starts_with("Parent section")).unwrap().target.unwrap();let start=edited.find("content").unwrap();let expected=range_of(&edited,start,start+"content".len());assert_eq!((definition.range,marker.range),(expected,expected));
}
#[test]
fn registered_layout_members_do_not_read_or_navigate_separately_owned_target_source() {
    let host=MockHost::install();let mut plugin=activated(&host);host.add_project("/project/resources/views/layouts");let (source,at)=cursor("@extends('layouts.app')\n@section('con|tent')");let d=doc("/project/resources/views/new.blade.php","blade",&source);
    let observed=(plugin.completion(&d,at,None).len(),plugin.definition(&d,at).len(),plugin.line_markers(&d).into_iter().filter(|m|m.tooltip.starts_with("Parent section")).count());assert_eq!(observed,(0,0,0));assert!(host.requests().is_empty() && host.process_runs().is_empty());
}

#[test]
fn registered_blade_associations_skip_comment_data_but_accept_literal_argument_trivia() {
    let host=MockHost::install();laravel_project(&host);host.add_file("resources/views/fake-comment.blade.php","@if(/* ) @include('posts.index') */ $real) @endif");host.add_file("resources/views/commented-real.blade.php","@include(/* 'fake', ) => */ 'posts.index' /* , */)");let mut plugin=Main::default();plugin.activate(&host.context("aritti.laravel")).unwrap();crate::tests::finish_indexing(&mut plugin);let found=plugin.line_markers(&doc("/project/resources/views/posts/index.blade.php","blade","<div>Post</div>"));let used=found.iter().find(|m|m.tooltip.starts_with("Used by")).map(|m|m.tooltip.as_str()).unwrap_or("");assert!(used.contains("commented-real") && !used.contains("fake-comment"),"{found:?}");
}
#[test]
fn registered_layout_definition_accepts_literal_comments_with_exact_utf16_source_range() {
    let host=MockHost::install();laravel_project(&host);let target="😀 @yield(/* 'fake' , ) */ 'hé😀' /* , */)";host.add_file("resources/views/layouts/comments.blade.php",target);let mut plugin=Main::default();plugin.activate(&host.context("aritti.laravel")).unwrap();crate::tests::finish_indexing(&mut plugin);let(source,at)=cursor("@extends('layouts.comments')\n@section('hé|😀')");let result=plugin.definition(&doc("/project/resources/views/new.blade.php","blade",&source),at);let start=target.find("hé😀").unwrap();assert_eq!(result.len(),1,"{result:?}");assert_eq!(result[0].range,range_of(target,start,start+"hé😀".len()));
}

#[test]
fn registered_dynamic_ability_binding_and_column_definitions_select_utf16_source_tokens() {
    let host=MockHost::install();laravel_project(&host);
    let provider="<?php\r\n/* 😀 */ Gate::define('can-café😀',fn()=>true);\r\n$this->app->bind('service-café😀',fn()=>null);\r\n";let migration="<?php\r\nSchema::table('posts',function(Blueprint $t){ $t->string('café😀'); });\r\n";
    host.add_file("app/Providers/UnicodeServiceProvider.php",provider);host.add_file("database/migrations/2024_01_05_unicode_posts.php",migration);let mut plugin=Main::default();plugin.activate(&host.context("aritti.laravel")).unwrap();crate::tests::finish_indexing(&mut plugin);
    let observed:Vec<_>=[("ability","<?php Gate::allows('can-caf|é😀');",provider,"can-café😀"),("binding","<?php app('service-caf|é😀');",provider,"service-café😀"),("column","<?php Post::where('caf|é😀',1);",migration,"café😀")].into_iter().map(|(kind,marked,target,name)|{let(source,at)=cursor(marked);let found=plugin.definition(&doc("/project/test.php","php",&source),at);assert_eq!(found.len(),1,"{kind}: {found:?}");let start=target.find(name).unwrap();(kind,found[0].range,range_of(target,start,start+name.len()))}).collect();
    assert!(observed.iter().all(|(_,actual,expected)|actual==expected),"{observed:?}");assert!(host.requests().is_empty() && host.process_runs().is_empty());
}

#[test]
fn registered_literal_definition_locations_refuse_new_foreign_target_owners() {
    let observed:Vec<_>=[("resources/views","php","<?php view('posts.in|dex');"),("config","php","<?php config('app.na|me');"),("lang","php","<?php __('messages.wel|come');"),("routes","php","<?php route('ho|me');"),("routes","typescript","route('ho|me')")].into_iter().map(|(target,language,marked)|{let host=MockHost::install();let mut plugin=activated(&host);let path=if language=="php"{"/project/test.php"}else{"/project/main.ts"};let(source,at)=cursor(marked);let d=doc(path,language,&source);assert!(!plugin.definition(&d,at).is_empty(),"positive {marked}");host.add_project(&format!("/project/{target}"));(target,language,plugin.definition(&d,at).len())}).collect();assert!(observed.iter().all(|(_,_,n)|*n==0),"{observed:?}");
}
#[test]
fn registered_gutter_targets_do_not_cross_current_owners_but_keep_unaffected_links() {
    let host=MockHost::install();let mut plugin=activated(&host);let source=host_text(&host,"app/Http/Controllers/PostController.php");let d=doc("/project/app/Http/Controllers/PostController.php","php",&source);assert!(plugin.line_markers(&d).iter().any(|m|m.target.as_ref().is_some_and(|t|t.path.contains("/routes/"))));host.add_project("/project/routes");let markers=plugin.line_markers(&d);assert!(!markers.iter().any(|m|m.target.as_ref().is_some_and(|t|t.path.contains("/routes/"))),"{markers:?}");assert!(markers.iter().any(|m|m.tooltip=="View posts.index"));
    let host=MockHost::install();let mut plugin=activated(&host);host.add_project("/project/app/Http/Controllers");let d=doc("/project/resources/views/posts/index.blade.php","blade","<div>Post</div>");assert!(!plugin.line_markers(&d).iter().any(|m|m.tooltip.starts_with("Rendered by")));
}
#[test]
fn registered_checked_related_navigation_refuses_new_foreign_factory_owner() {
    let host=MockHost::install();host.add_file("database/factories/PostFactory.php","<?php class PostFactory extends Factory {}");let mut plugin=activated(&host);let usage="<?php class FactoryUsage { function run() { Post::factory(); } }";let d=doc("/project/app/Http/Controllers/FactoryUsage.php","php",usage);let at=text::position(usage,usage.find("Post::").unwrap()+1);assert!(!plugin.definition(&d,at).is_empty());host.add_project("/project/database/factories");assert!(plugin.definition(&d,at).is_empty());assert!(host.requests().is_empty() && host.process_runs().is_empty());
}

#[test]
fn registered_component_props_slots_and_livewire_members_refuse_new_foreign_source_owners() {
    let observed:Vec<_>=[("anonymous","<x-banner |","/project/resources/views/components"),("slot","<x-banner><x-slot:fo|","/project/resources/views/components"),("class","<x-alert |","/project/app/View/Components"),("livewire","<livewire:tally |","/project/app/Livewire")].into_iter().map(|(kind,marked,child)|{let host=MockHost::install();laravel_project(&host);host.add_file("resources/views/components/banner.blade.php","@props(['tone' => 'info']) <div>{{ $footer }}</div>");host.add_file("app/View/Components/Alert.php","<?php\nclass Alert extends Component\n{\n public function __construct(public string $tone = 'info'){}\n}\n");host.add_file("app/Livewire/Tally.php","<?php\nclass Tally extends Component\n{\n public int $count = 0;\n}\n");let mut plugin=Main::default();plugin.activate(&host.context("aritti.laravel")).unwrap();crate::tests::finish_indexing(&mut plugin);let(source,at)=cursor(marked);let d=doc("/project/resources/views/x.blade.php","blade",&source);assert!(!plugin.completion(&d,at,None).is_empty(),"positive {kind}");host.add_project(child);(kind,labels(plugin.completion(&d,at,None)))}).collect();assert!(observed.iter().all(|(_,names)|names.is_empty()),"{observed:?}");
}
#[test]
fn owned_component_source_refreshes_current_properties_without_replacing_root_cache() {
    let host=MockHost::install();host.add_file("resources/views/components/banner.blade.php","@props(['tone'])");let mut plugin=activated(&host);let(source,at)=cursor("<x-banner |");let d=doc("/project/resources/views/x.blade.php","blade",&source);assert_eq!(labels(plugin.completion(&d,at,None)),["tone"]);host.open_editor("/project/resources/views/components/banner.blade.php","blade","@props(['fresh'])",&[]);assert_eq!(labels(plugin.completion(&d,at,None)),["fresh"]);assert!(plugin.indexes.contains_key("/project"));assert!(host.requests().is_empty() && host.process_runs().is_empty());
}

#[test]
fn endpoints_resolve_application_mounts_and_refresh_configuration() {
    use openidea_plugin::endpoints::EndpointsProvider;
    let host = MockHost::install();
    let mut plugin = activated(&host);
    host.add_file("routes/api.php", "<?php Route::post('/snatch/edit', [SnatchController::class, 'edit']); Route::prefix('admin')->group(function(){ Route::apiResource('posts', PostController::class)->only(['index']); });");
    for (prefix, expected) in [("'api'", Some("/api/snatch/edit")), ("'v2'", Some("/v2/snatch/edit")), ("''", Some("/snatch/edit")), ("$dynamic", None)] {
        host.add_file("bootstrap/app.php", &format!("<?php return Application::configure()->withRouting(web: __DIR__.'/../routes/web.php', api: __DIR__.'/../routes/api.php', apiPrefix: {prefix});"));
        plugin.handle_events(&[Event::FileSaved("/project/bootstrap/app.php".into())], &host.invocation());crate::tests::finish_indexing(&mut plugin);
        let endpoints = plugin.endpoints(&host.invocation()).unwrap();
        let api: Vec<_> = endpoints.iter().filter(|e| e.location.path.ends_with("/routes/api.php")).collect();
        match expected {
            Some(expected) => {
                let e = api.iter().find(|e| e.path == expected).expect("mounted issue411 route");
                assert_eq!(e.handler.as_deref(), Some("SnatchController@edit"));
                assert_eq!(e.method.as_deref(), Some("POST"));
                let base = expected.strip_suffix("/snatch/edit").unwrap();
                assert!(api.iter().any(|e| e.path == format!("{base}/admin/posts")));
            }
            None => assert!(api.is_empty()),
        }
    }
    host.add_file("bootstrap/app.php", "<?php return Application::configure()->withRouting(web: __DIR__.'/../routes/web.php');");
    plugin.handle_events(&[Event::FileSaved("/project/bootstrap/app.php".into())], &host.invocation());crate::tests::finish_indexing(&mut plugin);
    assert!(plugin.endpoints(&host.invocation()).unwrap().iter().all(|e| !e.location.path.ends_with("/routes/api.php")));
}

#[test]
fn endpoints_watch_legacy_provider_prefix_changes() {
    use openidea_plugin::endpoints::EndpointsProvider;
    let h=MockHost::install();let mut plugin=activated(&h);
    h.add_file("routes/api.php","<?php Route::post('/snatch/edit', 'SnatchController@edit');");
    for prefix in ["api","v3"] {
        h.add_file("app/Providers/RouteServiceProvider.php",&format!("<?php Route::middleware('api')->prefix('{prefix}')->group(base_path('routes/api.php'));"));
        plugin.handle_events(&[Event::FileSaved("/project/app/Providers/RouteServiceProvider.php".into())],&h.invocation());crate::tests::finish_indexing(&mut plugin);
        assert!(plugin.endpoints(&h.invocation()).unwrap().iter().any(|e|e.path==format!("/{prefix}/snatch/edit")));
    }
    assert!(h.requests().is_empty() && h.process_runs().is_empty());
}

#[test]
fn native_bulk_phpdoc_action_refuses_changed_source_and_generators_refuse_collisions() {
    use openidea_plugin::file_actions::FileActions;
    let host=MockHost::install().with_capabilities(&["project.read","project.write","events"]);
    let text="<?php\n/**\n * @property int $id\n */\nclass Tag extends Model {}\n";
    host.add_file("app/Models/Tag.php",text);
    let mut plugin=activated(&host);
    let doc=openidea_plugin::providers::Document { path:"/project/app/Models/Tag.php".into(),language:"php".into(),text:text.into(),version:1 };
    let action=plugin.file_actions(&doc,Range::new(Position::new(0,0),Position::new(0,0))).remove(0);
    host.add_file("app/Models/Tag.php","<?php class Tag extends Model { /* changed */ }");
    assert!(plugin.resolve_file_action(&doc,action).is_err());
    host.add_file("database/factories/PostFactory.php","existing");
    let index=plugin.indexes.get("/project").unwrap();
    assert!(bulk::model_file(&host.invocation().project().unwrap(),index,"factory","Post","PostFactory").is_err());
    assert_eq!(host.invocation().project().unwrap().read_file("database/factories/PostFactory.php").unwrap(),"existing");
    assert!(host.process_runs().is_empty());
}

#[test]
fn packaged_resource_tabs_include_icons_and_cached_inventory_counts() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let ctx = host.invocation();
    let payload = serde_json::json!({"channel":"ui-kit","op":"render","sequence":1,"frameId":"0123456789abcdef0123456789abcdef","project":"/project"});
    plugin.handle(VIEW, ViewEvent::Message(payload.to_string()), &ctx).unwrap();
    let messages = host.view_messages();
    let reply: serde_json::Value = serde_json::from_str(&messages.last().unwrap().1).unwrap();
    let tabs = reply["model"]["viewTabs"].as_array().expect("bridge tabs");
    let index = plugin.view_index(&ctx).unwrap();
    assert_eq!(tabs.len(), 15);
    for (id, count) in [(VIEW, index.route_defs.len()), ("laravel-views", index.views.len()), ("laravel-components", index.components.len()+index.livewire.len()), ("laravel-config", index.config.len()), ("laravel-translations", index.translations_by_locale.values().map(Names::len).sum()), ("laravel-middleware", index.middleware.len()), ("laravel-env", index.env.len())] {
        let tab = tabs.iter().find(|tab|tab["id"] == id).unwrap();
        assert!(tab["icon"].as_str().is_some_and(|icon| !icon.is_empty()));
        assert_eq!(tab["count"].as_u64(), Some(count as u64));
    }
    assert_eq!(tabs[0]["id"], "laravel-about");
    let icons: std::collections::BTreeSet<_> = tabs.iter().filter_map(|tab|tab["icon"].as_str()).collect();
    assert_eq!(icons.len(), tabs.len(), "Tabs have distinct purpose-specific icons");
}

#[test]
fn resource_tabs_offer_only_relevant_toolbar_actions() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let ctx = host.invocation();
    for (tab, cache) in [(VIEW, Some("route-clear")), ("laravel-views", Some("view-clear")), ("laravel-config", Some("config-clear")), ("laravel-env", Some("config-clear")), ("laravel-translations", None), ("laravel-middleware", None)] {
        let view = plugin.render(tab, None, &ctx).unwrap();
        let ids: Vec<_> = view.toolbar.iter().map(|button| button.id.as_str()).collect();
        assert_eq!(ids.contains(&"reindex"), tab != VIEW, "{tab}");
        assert_eq!(ids.contains(&"routes-runtime-refresh"), tab == VIEW, "{tab}");
        assert_eq!(ids.contains(&"route-list"), tab == VIEW, "{tab}");
        assert!(!ids.contains(&"optimize-clear"), "{tab}");
        for id in ["route-clear", "view-clear", "config-clear"] {
            assert_eq!(ids.contains(&id), cache == Some(id), "{tab}/{id}");
        }
    }
}

#[test]
fn run_artisan_command_reveals_workspace_without_execution() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    plugin.run_command("artisan-run", &host.invocation(), None).unwrap();
    assert_eq!(host.revealed_views(), [VIEW]);
    assert!(host.process_runs().is_empty());
    let config: serde_json::Value = serde_json::from_str(include_str!("ui_config.json")).unwrap();
    assert!(config["group"]["tabs"].as_array().unwrap().iter().any(|tab| tab["id"] == artisan_workspace::VIEW));
}

#[test]
fn translation_inventory_preserves_all_five_locales_and_opens_each_definition() {
    let host = MockHost::install();
    let files: Vec<(String, String)> = [("de", "Windeln"), ("en", "Diapers"), ("es", "Pañales"), ("fr", "Couches"), ("nl", "Luiers")]
        .into_iter().map(|(locale, value)| (format!("lang/{locale}/enums.php"), format!("<?php return ['affiliate_category' => ['diapers' => '{value}']];"))).collect();
    let borrowed: Vec<_> = files.iter().map(|(path,text)| (path.as_str(),text.as_str())).collect();
    let mut plugin = activated_with(&host, &borrowed);
    let ctx = host.invocation();
    let index = plugin.view_index(&ctx).unwrap();
    assert_eq!(index.translations["enums.affiliate_category.diapers"].detail.as_deref(), Some("Diapers [en]"), "completion still prefers the main locale");
    let view = plugin.render("laravel-translations", None, &ctx).unwrap();
    let Body::Other {kind,data} = view.body else {panic!("expected translation resources")};
    assert_eq!(kind,"resources");
    let model: serde_json::Value = serde_json::from_str(&data).unwrap();
    assert_eq!(model["columns"],serde_json::json!(["Key","Locale","Value","File"]));
    assert_eq!(model["columnStyles"],serde_json::json!(["code","locale","text","muted"]));
    assert_eq!(model["facets"],serde_json::json!([1]));
    let matches: Vec<_> = model["items"].as_array().unwrap().iter().filter(|row|row["cells"][0]=="enums.affiliate_category.diapers").collect();
    assert_eq!(matches.len(),5);
    let locales: Vec<_> = matches.iter().map(|row|row["cells"][1].as_str().unwrap()).collect();
    assert_eq!(locales,["de","en","es","fr","nl"]);
    for row in matches {
        let key=row["key"].as_str().unwrap();let locale=row["cells"][1].as_str().unwrap();
        assert_eq!(row["cells"][3],format!("lang/{locale}/enums.php"));
        plugin.handle("laravel-translations", ViewEvent::Activate(key.into()), &ctx).unwrap();
        let opened=host.opened_locations();
        assert_eq!(opened.last().unwrap().0,format!("/project/lang/{locale}/enums.php"));
    }
    let config:serde_json::Value=serde_json::from_str(&plugin.web_config(&ctx)).unwrap();
    let tab=config["group"]["tabs"].as_array().unwrap().iter().find(|tab|tab["id"]=="laravel-translations").unwrap();
    assert_eq!(tab["count"].as_u64(),Some(model["items"].as_array().unwrap().len() as u64));
}

#[test]
fn translation_inventory_includes_json_and_vendor_locales_without_collapsing_keys() {
    let host=MockHost::install();
    let plugin=activated_with(&host,&[
        ("lang/en.json",r#"{"Say \"hi\": 🦀":"Hello"}"#),
        ("lang/fr.json",r#"{"Say \"hi\": 🦀":"Bonjour"}"#),
        ("lang/vendor/shop/en/messages.php","<?php return ['hello'=>'Hello shop'];"),
        ("lang/vendor/shop/fr/messages.php","<?php return ['hello'=>'Bonjour boutique'];"),
    ]);
    let index=plugin.view_index(&host.invocation()).unwrap();
    for (locale,value) in [("en","Hello"),("fr","Bonjour")] {
        let entry=&index.translations_by_locale[locale]["Say \"hi\": 🦀"];
        assert_eq!(entry.detail.as_deref(),Some(value));
        let key=format!("translation-locale:{}",serde_json::json!([locale,"Say \"hi\": 🦀"]));
        assert_eq!(entry_for(index,&key).unwrap().0,format!("/project/lang/{locale}.json"));
    }
    assert_eq!(index.translations_by_locale["fr"]["shop::messages.hello"].detail.as_deref(),Some("Bonjour boutique"));
    assert!(entry_for(index,"translation-locale:invalid").is_none());
    assert!(entry_for(index,r#"translation-locale:["missing","messages.hello"]"#).is_none());
}

#[test]
fn saving_a_locale_rebuilds_its_inventory_and_retires_removed_definitions() {
    let host=MockHost::install();let mut plugin=activated_with(&host,&[("lang/de/labels.php","<?php return ['hello'=>'Hallo'];")]);
    let key=format!("translation-locale:{}",serde_json::json!(["de","labels.hello"]));
    assert!(entry_for(plugin.view_index(&host.invocation()).unwrap(),&key).is_some());
    host.add_file("lang/de/labels.php","<?php return ['bye'=>'Tschüss'];");
    plugin.handle_events(&[Event::FileSaved("/project/lang/de/labels.php".into())],&host.invocation());crate::tests::finish_indexing(&mut plugin);
    let index=plugin.view_index(&host.invocation()).unwrap();assert!(!index.translations_by_locale["de"].contains_key("labels.hello"));
    assert_eq!(index.translations_by_locale["de"]["labels.bye"].detail.as_deref(),Some("Tschüss"));
    plugin.handle("laravel-translations",ViewEvent::Activate(key),&host.invocation()).unwrap();
    assert!(host.opened_locations().is_empty());
}

#[test]
fn registered_eloquent_members_use_owned_indexes_and_refresh_after_model_edits() {
    let host=MockHost::install();
    let source=r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class Purchase extends Model {
 public function customer(){return $this->belongsTo(Customer::class);}
}
"#;
    let customer=r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class Customer extends Model {}
"#;
    let mut plugin=activated_with(&host,&[
        ("app/Models/Purchase.php",source),("app/Models/Customer.php",customer),
        ("database/migrations/100_tables.php","<?php Schema::create('purchases',function($table){$table->id();$table->string('reference');}); Schema::create('customers',function($table){$table->id();$table->string('email');});"),
    ]);
    let marked=r#"<?php use App\Models\Purchase as Order; function show(Order $order){$order->customer->em^ail;}"#;
    let (text,at)=cursor(marked);let document=doc("/project/app/Http/Controllers/OrderController.php","php",&text);
    let items=plugin.completion(&document,at,None);assert!(items.iter().any(|item|item.label=="email"));
    let locations=plugin.definition(&document,at);assert_eq!(locations.len(),1);assert!(locations[0].path.ends_with("100_tables.php"));
    assert!(host.process_runs().is_empty());assert!(host.requests().is_empty());
    host.add_file("app/Models/Purchase.php",source.replace("customer()","buyer()").as_str());
    plugin.handle_events(&[Event::FileSaved("/project/app/Models/Purchase.php".into())],&host.invocation());crate::tests::finish_indexing(&mut plugin);
    assert!(plugin.completion(&document,at,None).is_empty(),"removed relation must not remain inferred");
    let (text,at)=cursor(r#"<?php use App\Models\Purchase; Purchase::first()->buyer->^"#);let updated=doc(&document.path,"php",&text);
    assert!(plugin.completion(&updated,at,None).iter().any(|item|item.label=="email"));
    host.add_project("/project/app/Models");
    assert!(plugin.completion(&updated,at,None).is_empty(),"new foreign model owner must not leak cached members");
    assert!(host.process_runs().is_empty());
}

#[test]
fn artisan_workspace_errors_notify_once_and_preview_validation_stays_in_form() {
    let host=MockHost::install().with_capabilities(&["project.read","storage","events"]);
    host.add_file("artisan","<?php");
    let mut plugin=activated(&host);let ctx=host.invocation();
    host.answer_next_prompt(Some(&["ok"]));
    plugin.handle(artisan_workspace::VIEW,ViewEvent::Message(r#"{"op":"refresh"}"#.into()),&ctx).unwrap();
    let notes=host.notifications();assert_eq!(notes.len(),1);assert_eq!(notes[0].title,"Artisan");
    assert!(notes[0].body.contains("process:php"));
    let messages=host.view_messages();
    let envelope:serde_json::Value=serde_json::from_str(&messages.last().unwrap().1).unwrap();
    let response:serde_json::Value=serde_json::from_str(envelope["message"].as_str().unwrap()).unwrap();
    assert!(response["error"].as_str().is_some());
    plugin.handle(artisan_workspace::VIEW,ViewEvent::Message(r#"{"op":"preview","command":"about","token":"stale","values":{},"revision":1}"#.into()),&ctx).unwrap();
    assert_eq!(host.notifications().len(),1,"typing validation must not create notifications");
    assert!(host.process_runs().is_empty());
}

#[test]
fn config_inventory_displays_source_expressions_and_preserves_navigation_and_literal_facts() {
    let host=MockHost::install();
    host.add_file("config/affiliate.php",r#"<?php return ['associate_tags'=>['fr'=>env('AMAZON_TAG_FR'),'en'=>env('AMAZON_TAG_EN', '')],'enabled'=>true,'count'=>5,'nothing'=>null,'empty'=>'','literal'=>'shop','computed'=>base_path('shop').'/data','long'=>'😀😀'];"#);
    let mut plugin=activated(&host);let ctx=host.invocation();
    let index=plugin.view_index(&ctx).unwrap();
    assert_eq!(index.config["affiliate.literal"].detail.as_deref(),Some("shop"));
    assert_eq!(index.config["affiliate.associate_tags.fr"].detail,None);
    let source=index.config["affiliate.associate_tags.fr"].clone();
    let view=plugin.render("laravel-config",None,&ctx).unwrap();
    let Body::Other{kind,data}=view.body else {panic!("expected Config table")};assert_eq!(kind,"resources");
    let table:serde_json::Value=serde_json::from_str(&data).unwrap();
    assert_eq!(table["columns"],serde_json::json!(["Key","Value","File"]));assert_eq!(table["facets"],serde_json::json!([2]));
    for (key,value) in [("associate_tags","array"),("associate_tags.fr","env('AMAZON_TAG_FR')"),("enabled","true"),("count","5"),("nothing","null"),("empty","''"),("literal","'shop'"),("computed","base_path('shop').'/data'")] {
        let row=table["items"].as_array().unwrap().iter().find(|row|row["key"]==format!("config:affiliate.{key}")).unwrap();
        assert_eq!(row["cells"][1],value);assert_eq!(row["cells"][2],"config/affiliate.php");
    }
    plugin.handle("laravel-config",ViewEvent::Activate("config:affiliate.associate_tags.fr".into()),&ctx).unwrap();
    let locations=host.opened_locations();assert_eq!(locations.last().unwrap(),&(source.path,source.at.line,source.at.character));
    assert!(host.process_runs().is_empty());
}

#[test]
fn config_expression_previews_are_bounded_utf8_safe_and_follow_first_definition() {
    let host=MockHost::install();
    host.add_file("config/previews.php",&format!("<?php return ['duplicate'=>'first','duplicate'=>'second','long'=>'{}','comment'=>/* note */ env('APP_NAME')];", "😀".repeat(600)));
    let plugin=activated(&host);let index=plugin.view_index(&host.invocation()).unwrap();
    assert_eq!(index.config_expressions["previews.duplicate"],"'first'");
    assert_eq!(index.config_expressions["previews.long"].chars().count(),513);
    assert!(index.config_expressions["previews.long"].ends_with('…'));
    assert_eq!(index.config_expressions["previews.comment"],"env('APP_NAME')");
    assert!(host.process_runs().is_empty());
}

#[test]
fn config_numeric_paths_reach_inventory_completion_and_source_navigation() {
    let host=MockHost::install();host.add_file("config/child.php",r#"<?php return ['schedule'=>[['name'=>'First'],['name'=>'Second']]];"#);
    let mut plugin=activated(&host);let ctx=host.invocation();
    let index=plugin.view_index(&ctx).unwrap();assert!(index.config.contains_key("child.schedule.1.name"));
    let entry=index.config["child.schedule.1.name"].clone();
    let (source,at)=cursor("<?php config('child.schedule.1.na^');");
    assert!(plugin.completion(&doc("/project/test.php","php",&source),at,None).iter().any(|item|item.label=="child.schedule.1.name"));
    plugin.handle("laravel-config",ViewEvent::Activate("config:child.schedule.1.name".into()),&ctx).unwrap();
    assert_eq!(host.opened_locations().last().unwrap(),&(entry.path,entry.at.line,entry.at.character));
    assert!(host.process_runs().is_empty());
}
#[test]
fn config_unsupported_arrays_report_partial_coverage_without_discarding_known_keys() {
    let host=MockHost::install();host.add_file("config/dynamic.php","<?php return [...$extra,'known'=>['child'=>true]];");
    let mut plugin=activated(&host);let ctx=host.invocation();
    assert!(plugin.view_index(&ctx).unwrap().config.contains_key("dynamic.known.child"));
    assert!(!missing_config(plugin.view_index(&ctx).unwrap(),"dynamic.known.unknown"));
    let view=plugin.render("laravel-config",None,&ctx).unwrap();
    assert!(view.status.as_deref().unwrap().contains("Config coverage incomplete"));
    assert!(!plugin.view_index(&ctx).unwrap().partial,"Config uncertainty must not suppress unrelated diagnostics");
}
#[test]
fn unknown_keys_confined_to_one_array_keep_other_config_diagnostics_authoritative() {
    let host=MockHost::install();
    host.add_file("config/site.php","<?php return ['name'=>'x','previous_keys'=>[...array_filter(explode(',',env('K')))],'nested'=>['kept'=>['leaf'=>1]]];");
    host.add_file("config/depot.php","<?php return ['default'=>'local','links'=>[public_path('storage')=>storage_path('app/public')],'disks'=>['local'=>['driver'=>'local']]];");
    host.add_file("config/dynamic.php","<?php return [...$extra,'known'=>['child'=>true]];");
    let mut plugin=activated(&host);let ctx=host.invocation();
    let index=plugin.view_index(&ctx).unwrap();
    assert!(missing_config(index,"site.absent"));
    assert!(missing_config(index,"site.nested.kept.absent"));
    assert!(missing_config(index,"depot.disks.local.absent"));
    assert!(!missing_config(index,"site.previous_keys.0"));
    assert!(!missing_config(index,"depot.links.anything"));
    assert!(!missing_config(index,"dynamic.known.unknown"), "a top-level spread can overwrite any sibling");
    assert!(!missing_config(index,"dynamic.absent"));
    assert_eq!(index.config_coverage_gaps(),["depot.links","dynamic","site.previous_keys"]);
    let view=plugin.render("laravel-config",None,&ctx).unwrap();
    let status=view.status.unwrap();
    assert!(status.contains("site.previous_keys") && status.contains("depot.links"),"{status}");
}
#[test]
#[ignore="requires LARAVEL_CONFIG_FIXTURE_DIR containing actual config PHP files"]
fn captured_config_directory_coverage() {
    let dir=std::path::PathBuf::from(std::env::var("LARAVEL_CONFIG_FIXTURE_DIR").unwrap());
    let mut previous=0;let mut current=0;let mut files=0;let mut incomplete=vec![];
    for entry in std::fs::read_dir(dir).unwrap() {
        let path=entry.unwrap().path();if path.extension().and_then(|e|e.to_str())!=Some("php"){continue;}
        let text=std::fs::read_to_string(&path).unwrap();let masked=php::mask_comments(&text);let name=path.file_stem().unwrap().to_str().unwrap();
        let mut old=vec![];if let Some(open)=php::returned_array(&masked){php::array_paths(&masked,open,name,&mut old,0);}
        let (new,complete)=config_arrays::paths(&masked,name);previous+=old.len()+1;current+=new.len()+1;files+=1;
        if !complete{incomplete.push(name.to_owned());}
    }
    println!("Config source audit: {files} files, {previous} previous keys → {current} keys; incomplete files: {incomplete:?}");
    assert!(current>previous,"list fixture should gain numeric keys");
}

#[test]
fn runtime_routes_drive_tabs_actions_navigation_and_expire_on_save() {
    let host=MockHost::install();host.add_file("artisan","<?php");let mut plugin=activated(&host);let ctx=host.invocation();
    let raw=r#"[{"method":"GET|HEAD","uri":"api/live","name":"live","action":"App\\Http\\Controllers\\PostController@index"}]"#;
    let snapshot=route_runtime::capture_with(&ctx,plugin.view_index(&ctx).unwrap(),|_|Ok(raw.into())).unwrap();
    plugin.runtime_routes.insert("/project".into(),snapshot);
    let endpoints=openidea_plugin::endpoints::EndpointsProvider::endpoints(&mut plugin,&ctx).unwrap();
    assert!(endpoints.iter().any(|endpoint|endpoint.path=="/api/live" && endpoint.method.as_deref()==Some("GET")));
    assert!(endpoints.iter().any(|endpoint|endpoint.path=="/api/live" && endpoint.method.as_deref()==Some("HEAD")));
    let view=plugin.render(VIEW,None,&ctx).unwrap();assert_eq!(route_rows(&view)[0].1,"/api/live");assert!(view.status.unwrap().contains("Artisan snapshot"));
    assert_eq!(plugin.route_at("route:0",&ctx).unwrap().path,"/api/live");
    let config:serde_json::Value=serde_json::from_str(&plugin.web_config(&ctx)).unwrap();assert_eq!(config["group"]["tabs"].as_array().unwrap().iter().find(|tab|tab["id"] == VIEW).unwrap()["count"],1);
    plugin.handle(VIEW,ViewEvent::Activate("route:0".into()),&ctx).unwrap();assert!(host.opened_locations().last().unwrap().0.ends_with("PostController.php"));
    plugin.handle_events(&[Event::FileSaved("/project/routes/web.php".into())],&ctx);crate::tests::finish_indexing(&mut plugin);assert!(plugin.runtime_routes.is_empty());
    assert!(plugin.render(VIEW,None,&ctx).unwrap().status.unwrap().contains("Routes read from source files"));
    assert!(host.process_runs().is_empty());
}
#[test]
fn runtime_route_failure_retains_previous_snapshot_and_static_dynamic_uris_are_not_faked() {
    let host=MockHost::install().with_capabilities(&["project.read","events"]);host.add_file("artisan","<?php");let mut plugin=activated(&host);let ctx=host.invocation();
    let routes=route_runtime::parse(r#"[{"method":"GET","uri":"kept","name":null,"action":"Closure"}]"#,plugin.view_index(&ctx).unwrap()).unwrap();
    plugin.runtime_routes.insert("/project".into(),route_runtime::Snapshot::fixture(routes,1));
    host.add_file("config/newly_added.php","<?php return ['enabled'=>true];");
    assert!(!plugin.view_index(&ctx).unwrap().config.contains_key("newly_added.enabled"));
    assert!(plugin.run_command("routes-runtime-refresh",&ctx,None).is_err(),"source refresh is queued before PHP starts");
    crate::tests::finish_indexing(&mut plugin);
    assert!(matches!(plugin.run_command("routes-runtime-refresh",&ctx,None),Err(Error::Invalid(message)) if message.contains("process:php permission")));
    assert!(plugin.view_index(&ctx).unwrap().config.contains_key("newly_added.enabled"),"refresh must rebuild the source index before starting PHP");
    assert_eq!(plugin.active_routes(&ctx)[0].path,"/kept");
    assert!(routes::parse("<?php Route::get($uri, [$controller,'index']);Route::delete($uri.'/{id}',[$controller,'destroy']);").is_empty());
    let redirect=routes::parse("<?php Route::redirect('settings','/settings/profile');");assert_eq!(redirect[0].action,"Redirect → /settings/profile");assert!(redirect[0].methods.contains("DELETE"));
}

#[test]
fn runtime_http_request_uses_bound_project_when_no_source_location_exists() {
    let host=MockHost::install().with_capabilities(&["project.read","project.write","clipboard","events"]);let mut plugin=activated(&host);let ctx=host.invocation();host.add_project("/other");
    let routes=route_runtime::parse(r#"[{"method":"GET","uri":"vendor-status","name":null,"action":"Closure"}]"#,plugin.view_index(&ctx).unwrap()).unwrap();assert!(routes[0].location.is_none());
    plugin.runtime_routes.insert("/project".into(),route_runtime::Snapshot::fixture(routes,1));
    host.answer_next_prompt(Some(&["http://localhost:8000"]));
    plugin.run_command("route-create-http",&ctx,Some("route:0")).unwrap();
    assert!(host.invocation().project().unwrap().read_file("/project/http/get-vendor-status.http").unwrap().contains("GET http://localhost:8000/vendor-status"));
}

#[test]
fn artisan_layout_stylesheet_changes_retire_the_cached_layout_url() {
    let html=artisan_workspace::html();
    let revision=artisan_metadata::hash(include_str!("../../../crates/ui-kit/web/command-workspace.css"));
    assert!(html.contains(&format!("command-workspace.css?v={revision}")));
    assert!(html.contains("ui-menu ui-scroll-area"));
    assert!(html.contains("command-detail ui-scroll-area"));
}

#[test]
fn views_inventory_includes_files_physical_folders_namespaces_and_source_navigation() {
    let host=MockHost::install();
    host.add_file("resources/views/pdf/layout.blade.php","<main>PDF</main>");
    host.add_file("ide-blade.json",r#"{"namespaces":[{"namespace":"shop","path":"modules/shop/views"}]}"#);
    host.add_file("modules/shop/views/catalog/item.blade.php","<article>Item</article>");
    host.add_file("resources/views/vendor/mail/html/header.blade.php","<header>Mail</header>");
    let mut plugin=activated(&host);let ctx=host.invocation();
    let view=plugin.render("laravel-views",None,&ctx).unwrap();let Body::Other{kind,data}=view.body else{panic!("expected views table")};assert_eq!(kind,"resources");
    let body:serde_json::Value=serde_json::from_str(&data).unwrap();assert_eq!(body["columns"],serde_json::json!(["View","File","Folder","Namespace"]));assert_eq!(body["facets"],serde_json::json!([2,3]));
    let rows=body["items"].as_array().unwrap();
    for (key,file,folder,namespace) in [("welcome","resources/views/welcome.blade.php","(root)","app"),("pdf.layout","resources/views/pdf/layout.blade.php","pdf","app"),("mail::html.header","resources/views/vendor/mail/html/header.blade.php","vendor.mail.html","mail"),("shop::catalog.item","modules/shop/views/catalog/item.blade.php","catalog","shop")] {
        let row=rows.iter().find(|row|row["key"]==format!("view:{key}")).unwrap();assert_eq!(row["cells"],serde_json::json!([key,file,folder,namespace]));assert_eq!(row["icon"],"file-html");
    }
    plugin.handle("laravel-views",ViewEvent::Activate("view:mail::html.header".into()),&ctx).unwrap();assert_eq!(host.opened_locations().last().unwrap().0,"/project/resources/views/vendor/mail/html/header.blade.php");
    host.add_project("/project/resources/views/vendor");
    let view=plugin.render("laravel-views",None,&ctx).unwrap();let Body::Other{data,..}=view.body else{panic!()};let body:serde_json::Value=serde_json::from_str(&data).unwrap();assert!(!body["items"].as_array().unwrap().iter().any(|row|row["key"]=="view:mail::html.header"));
    assert!(host.process_runs().is_empty());
}

#[test]
fn components_inventory_preserves_precedence_and_owned_navigation() {
    let host = MockHost::install();
    host.add_file("app/Providers/ComponentServiceProvider.php", "<?php Blade::component('package-alert', Alert::class);");
    let mut plugin = activated(&host);
    let ctx = host.invocation();
    let body = |plugin: &mut Main| {
        let view = plugin.render("laravel-components", None, &ctx).unwrap();
        assert_eq!(view.toolbar.len(), 1);
        assert_eq!(view.toolbar[0].id, "reindex");
        let Body::Other { kind, data } = view.body else { panic!("expected component inventory") };
        assert_eq!(kind, "resources");
        serde_json::from_str::<serde_json::Value>(&data).unwrap()
    };
    let inventory = body(&mut plugin);
    assert_eq!(inventory["columns"], serde_json::json!(["Component", "Type", "File"]));
    let rows = inventory["items"].as_array().unwrap();
    for (key, tag, kind, path) in [
        ("component:alert", "<x-alert>", "Blade class", "app/View/Components/Alert.php"),
        ("component:card", "<x-card>", "Blade anonymous", "resources/views/components/card.blade.php"),
        ("component:package-alert", "<x-package-alert>", "Blade alias", "app/Providers/ComponentServiceProvider.php"),
        ("livewire:counter", "<livewire:counter>", "Livewire", "app/Livewire/Counter.php"),
    ] {
        let row = rows.iter().find(|row| row["key"] == key).unwrap();
        assert_eq!(row["cells"], serde_json::json!([tag, kind, path]));
        plugin.handle("laravel-components", ViewEvent::Activate(key.into()), &ctx).unwrap();
        assert_eq!(host.opened_locations().last().unwrap().0, format!("/project/{path}"));
    }
    assert_eq!(rows.iter().filter(|row| row["key"] == "component:alert").count(), 1);
    host.add_project("/project/app/Livewire");
    let inventory = body(&mut plugin);
    assert!(!inventory["items"].as_array().unwrap().iter().any(|row| row["key"] == "livewire:counter"));
    let opened = host.opened_locations().len();
    plugin.handle("laravel-components", ViewEvent::Activate("livewire:counter".into()), &ctx).unwrap();
    assert_eq!(host.opened_locations().len(), opened);
    assert!(host.process_runs().is_empty());
}

#[test]
fn new_model_command_opens_native_wizard_without_running_php() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let project = host.invocation().project().unwrap();
    host.add_file("artisan", "<?php");
    let definition = serde_json::json!({"commands":[{"name":"make:model","description":"Create model","definition":{"arguments":[{"name":"name","is_required":true}],"options":[{"name":"--migration","accept_value":false}]}}]});
    let commands = artisan_metadata::parse(&definition.to_string()).unwrap();
    let snapshot = serde_json::json!({"schema":2,"identity":artisan_metadata::identity(&project).unwrap(),"capturedAt":1,"commands":commands});
    host::storage::set(&format!("artisan-metadata-v2:{}", artisan_metadata::hash("/project")), &snapshot.to_string()).unwrap();
    plugin.run_command("new-model", &host.invocation(), None).unwrap();
    let reply = plugin.artisan_workspace.handle(&host.invocation(), r#"{"op":"state"}"#, &plugin.artisan_terminals).unwrap();
    assert!(reply["selectCommand"].is_null());
    let view = plugin.render(generator_workspace::VIEW, None, &host.invocation()).unwrap();
    assert!(matches!(view.body, Body::Html(ref html) if html.contains("data-generator-workspace")));
    assert!(host.view_messages().iter().any(|(_, message)| message.contains("ui-kit-refresh")));
    assert!(host.process_runs().is_empty());
    assert!(plugin.artisan_terminals.commands().is_empty());
}

#[test]
fn parity_workspaces_render_without_php_network_or_direct_writes() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let ctx = host.invocation();
    for view in ["laravel-about", "laravel-generators", "laravel-models", "laravel-processes"] {
        let rendered = plugin.render(view, None, &ctx).unwrap();
        assert!(!matches!(rendered.body, Body::Message(ref message) if message.is_empty()));
    }
    assert!(host.process_runs().is_empty());
    assert!(host.requests().is_empty());
    assert!(host.terminals().is_empty());
    let config: serde_json::Value = serde_json::from_str(&plugin.web_config(&ctx)).unwrap();
    for id in ["runtime-open-frame"] {
        let action = config["actions"].as_array().unwrap().iter().find(|action| action["id"] == id).unwrap();
        assert_eq!(action["keys"], serde_json::json!([]), "Cold captured-row actions must remain unavailable: {id}");
    }
}

#[test]
fn metadata_input_and_schema_changes_refresh_indexes() {
    for path in ["ide.json", ".idea/ide.json", ".openidea/laravel-generators.json", "database/schema/mysql-schema.sql", "vite.config.ts", "public/build/manifest.json", "composer.lock"] {
        assert!(affects_index(path), "{path}");
    }
}

#[test]
fn scope_attribute_intentions_require_supported_locked_framework_and_respect_setting() {
    let source = "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nclass Post extends Model {\n public function scopeActive($query) { return $query; }\n public function scopeRecent($query) { return $query; }\n}\n";
    let at = text::position(source, source.find("scopeActive").unwrap() + 2);
    let selection = Range::new(at, at);
    for (version, expected) in [(None, false), (Some("v10.48.0"), false), (Some("v12.5.9"), false), (Some("v12.6.0"), true), (Some("v13.0.0"), true)] {
        let host = MockHost::install();
        if let Some(version) = version {
            host.add_file("composer.lock", &serde_json::json!({"packages":[{"name":"laravel/framework","version":version}]}).to_string());
        }
        let mut plugin = activated_with(&host, &[("app/Models/Post.php", source)]);
        let document = doc("/project/app/Models/Post.php", "php", source);
        let found = plugin.code_actions(&document, selection);
        assert_eq!(found.iter().any(|action| action.title.starts_with("Use #[Scope]")), expected, "locked version {version:?}");
        host.set_setting("scopeAttributeIntentions", "false");
        assert!(!plugin.code_actions(&document, selection).iter().any(|action| action.title.starts_with("Use #[Scope]")));
        assert!(host.process_runs().is_empty());
    }
}

#[test]
fn generator_package_overrides_gate_fixed_launchers_before_prompts_or_execution() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let composer = r#"{"require":{"laravel/framework":"^12","laravel/nova":"^5"}}"#;
    host.add_file("composer.json", composer);
    host.add_file("ide.json", r#"{"packages":{"disabledPackages":["laravel/nova"],"enabledPackages":{"livewire/livewire":"3.6.4"}}}"#);
    let ctx = host.invocation();
    plugin.index_project(&ctx.project().unwrap());crate::tests::finish_indexing(&mut plugin);
    let index = plugin.view_index(&ctx).unwrap();
    assert!(generate::make(&plugin.artisan_terminals, &ctx, Some(index), Some("nova:resource"), composer).is_err());
    host.answer_next_prompt(None);
    assert!(generate::make(&plugin.artisan_terminals, &ctx, Some(index), Some("livewire"), composer).unwrap().is_none());
    assert!(host.terminals().is_empty() && host.process_runs().is_empty());
}

#[test]
fn disabled_livewire_directives_do_not_remove_core_or_explicit_custom_blade_names() {
    let host = MockHost::install();
    let mut plugin = activated_with(&host, &[("ide.json", r#"{"packages":{"disabledPackages":["livewire/livewire"]}}"#)]);
    let document = doc("/project/resources/views/welcome.blade.php", "blade", "@");
    let labels: Vec<_> = plugin.completion(&document, Position::new(0,1), Some("@")).into_iter().map(|item|item.label).collect();
    assert!(labels.contains(&"if".to_owned()));
    for name in ["livewire","livewireStyles","livewireScripts","livewireScriptConfig"] { assert!(!labels.contains(&name.to_owned()), "{name}"); }
    host.add_file("app/Providers/BladeServiceProvider.php", "<?php Blade::directive('livewire', function () { return ''; });");
    plugin.index_project(&host.invocation().project().unwrap());crate::tests::finish_indexing(&mut plugin);
    let labels: Vec<_> = plugin.completion(&document, Position::new(0,1), Some("@")).into_iter().map(|item|item.label).collect();
    assert!(labels.contains(&"livewire".to_owned()));
    assert!(!labels.contains(&"livewireStyles".to_owned()));
}

#[test]
fn packaged_native_generation_and_process_views_are_opaque() {
    let config: serde_json::Value = serde_json::from_str(include_str!("ui_config.json")).unwrap();
    for id in ["laravel-generators", "laravel-processes"] {
        assert!(config["opaque_views"].as_array().unwrap().iter().any(|item| item == id));
        assert!(config["group"]["tabs"].as_array().unwrap().iter().any(|tab| tab["id"] == id));
    }
    assert_eq!(config["context_buttons"]["laravel-runtime"], serde_json::json!([]));
}

#[test]
fn database_actions_are_wired_to_owned_rows_and_native_lifecycle() {
    let h=MockHost::install();let mut main=activated(&h);let ctx=h.invocation();let project=ctx.project().unwrap();
    let id=format!("laravel-{}",artisan_metadata::hash("testing"));
    project.upsert_data_source(&openidea_plugin::PluginDataSource{id:id.clone(),name:"Testing".into(),driver:"sqlite".into(),..Default::default()}).unwrap();
    let key=format!("runtime:database:{}:{id}",artisan_metadata::hash(&project.root().unwrap()));
    let body=main.runtime_inspection.body(&ctx,artisan_runner::Runner::Php).unwrap();
    let Body::Other{data,..}=body else{panic!("resource body")};
    assert!(data.contains(&key));
    let config:serde_json::Value=serde_json::from_str(&main.web_config(&ctx)).unwrap();
    for action in ["runtime-database-connect","runtime-database-remove"] {
        let value=config["actions"].as_array().unwrap().iter().find(|v|v["id"]==action).unwrap();
        assert_eq!(value["keys"],serde_json::json!([key.clone()]));
    }
    main.run_command("runtime-database-connect",&ctx,Some(&key)).unwrap();
    h.answer_next_prompt(Some(&["yes"]));main.run_command("runtime-database-remove",&ctx,Some(&id)).unwrap();
    assert!(project.data_sources().unwrap().is_empty());assert!(h.process_runs().is_empty());
}
#[test]
fn route_search_command_finds_fuzzy_name_and_navigates_without_php() {
    let h=MockHost::install();let mut main=activated(&h);h.answer_next_prompt(Some(&["adsh"]));h.answer_next_prompt(Some(&["0"]));
    main.run_command("route-search",&h.invocation(),None).unwrap();
    assert!(h.invocation().editor().unwrap().path().ends_with("routes/web.php"));assert!(h.process_runs().is_empty());
}

#[test]
fn generators_reply_with_error_when_no_laravel_index_is_available() {
    let h=MockHost::install();let mut main=Main::default();
    main.handle(generator_workspace::VIEW,ViewEvent::Message(r#"{"op":"state"}"#.into()),&h.invocation()).unwrap();
    assert!(h.view_messages().iter().any(|(_,message)|message.contains("error")));
    assert!(h.process_runs().is_empty());
}

#[test]
fn blade_class_conversion_requires_a_supported_locked_framework() {
    let source="<div class=\"btn btn-primary\">x</div>";
    for (version,expected) in [(None,false),(Some("v8.0.0"),false),(Some("v9.0.0"),true),(Some("v13.0.0"),true)] {
        let h=MockHost::install();if let Some(version)=version {h.add_file("composer.lock",&serde_json::json!({"packages":[{"name":"laravel/framework","version":version}]}).to_string());}
        let mut main=activated(&h);let titles=rewrite_titles(&mut main,NEW_BLADE,"blade",source,"btn");
        assert_eq!(titles.iter().any(|title|title.contains("class attribute")),expected,"{version:?}");
        assert!(h.process_runs().is_empty());
    }
}
#[test]
fn invokable_controller_route_literals_complete_and_navigate_to_invoke() {
    let h=MockHost::install();let mut main=activated_with(&h,&[("app/Http/Controllers/EditProfileController.php","<?php\nnamespace App\\Http\\Controllers;\nfinal class EditProfileController {\n public function __invoke() {}\n public function __construct() {}\n}\n")]);
    let source="<?php Route::get('/profile', 'App\\Http\\Controllers\\EditProfileController');";
    let document=doc("/project/routes/web.php","php",source);let at=text::position(source,source.find("EditProfile").unwrap()+3);
    let completion_source="<?php Route::get('/profile', 'EditProfileController');";
    let completions=main.completion(&doc("/project/routes/web.php","php",completion_source),text::position(completion_source,completion_source.find("EditProfile").unwrap()+3),None);
    assert!(completions.iter().any(|item|item.label=="EditProfileController"&&item.insert_text=="\\App\\Http\\Controllers\\EditProfileController"));
    let definitions=main.definition(&document,at);assert!(definitions.iter().any(|location|location.path.ends_with("EditProfileController.php") && location.range.start.line==3));
    assert!(!completions.iter().any(|item|item.label.contains("__construct")));assert!(h.process_runs().is_empty());
}

#[test]
fn fallback_layout_uses_first_existing_source_without_false_missing_warning() {
    let h=MockHost::install();let mut main=activated(&h);
    let source="@extendsFirst(['missing.layout','layouts.app'])\n@section('content')Hi@endsection";
    let document=doc("/project/resources/views/child.blade.php","blade",source);
    let at=text::position(source,source.find("content").unwrap()+2);
    assert!(main.completion(&document,at,None).iter().any(|item|item.label=="content"));
    let index=main.indexes.get("/project").unwrap();
    assert!(!problems(index,"blade",source,true).iter().any(|diagnostic|diagnostic.code.as_deref()==Some("missing-view")));
    assert!(h.process_runs().is_empty());
}
#[test]
fn console_run_marker_selects_only_cached_metadata_and_revalidates_source() {
    use openidea_plugin::language_features::LanguageFeatures;
    let h=MockHost::install();h.add_file("artisan","<?php");let mut main=activated(&h);
    let source="<?php\nnamespace App\\Console\\Commands;\nuse Illuminate\\Console\\Command;\nclass SendReport extends Command {\n protected $signature='reports:send {--dry-run}';\n}\n";
    let path="/project/app/Console/Commands/SendReport.php";h.add_file(path,source);h.open_editor(path,"php",source,&[]);
    let ctx=h.invocation();let project=ctx.project().unwrap();
    let commands=artisan_metadata::parse(&serde_json::json!({"commands":[{"name":"reports:send","description":"Send report","definition":{"arguments":[],"options":[{"name":"--dry-run","accept_value":false}]}}]}).to_string()).unwrap();
    host::storage::set(&format!("artisan-metadata-v2:{}",artisan_metadata::hash("/project")),&serde_json::json!({"schema":2,"identity":artisan_metadata::identity(&project).unwrap(),"capturedAt":1,"commands":commands}).to_string()).unwrap();
    h.set_setting("routeHints","false");let document=doc(path,"php",source);
    assert!(main.code_lenses(&document).iter().any(|lens|lens.command=="console-command-open"));
    let action=main.gutter_actions(&document).into_iter().find(|action|action.command=="console-command-open").unwrap();
    main.run_command("console-command-open",&ctx,action.argument.as_deref()).unwrap();
    let state=main.artisan_workspace.handle(&ctx,r#"{"op":"state"}"#,&main.artisan_terminals).unwrap();assert_eq!(state["selectCommand"],"reports:send");
    h.type_in_editor(" // changed");assert!(main.run_command("console-command-open",&ctx,action.argument.as_deref()).is_err());assert!(h.process_runs().is_empty());
}

#[test]
fn dynamic_fillable_does_not_block_phpdoc_updates_or_declared_key_display() {
    use openidea_plugin::file_actions::FileActions;
    let source="<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\n/** Handwritten.\n * @property string $manual\n */\nclass DynamicRecord extends Model {\n private const FIELDS = ['title'];\n protected $fillable = self::FIELDS;\n protected $primaryKey = 'record_key';\n}\n";
    let h=MockHost::install();let mut main=activated_with(&h,&[("app/Models/DynamicRecord.php",source)]);let project=h.invocation().project().unwrap();
    let document=doc("/project/app/Models/DynamicRecord.php","php",source);
    let rows=model_overview::rows(main.indexes.get("/project").unwrap(),&project);
    assert!(rows.iter().any(|row|row["cells"][0]=="App\\Models\\DynamicRecord" && row["cells"][2].as_str().is_some_and(|text|text.contains("record_key"))));
    let action=main.file_actions(&document,Range::new(Position::new(0,0),Position::new(0,0))).into_iter().find(|action|action.data.as_deref().is_some_and(|data|data.starts_with("model-native:docs:"))).unwrap();
    h.answer_next_prompt(Some(&["yes"]));let result=main.resolve_file_action(&document,action).unwrap();
    let updated=&result.edits[0].1[0].text;assert!(updated.contains("@openidea-laravel-generated-start"));assert!(updated.contains("Handwritten."));assert!(updated.contains("@property string $manual"));assert!(updated.contains("$fillable = self::FIELDS;"));
    assert_eq!(project.read_file(&document.path).unwrap(),source);assert!(h.process_runs().is_empty());
}

#[test]
fn models_toolbar_groups_only_name_real_toolbar_buttons() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    let ctx = host.invocation();
    let config: serde_json::Value = serde_json::from_str(&plugin.web_config(&ctx)).unwrap();
    let groups = config["toolbar_groups"]["laravel-models"].as_array().unwrap();
    assert!(!groups.is_empty());
    let rendered = plugin.render("laravel-models", None, &ctx).unwrap();
    let ids: Vec<_> = rendered.toolbar.iter().map(|button| button.id.as_str()).collect();
    for group in groups {
        for item in group["items"].as_array().unwrap() {
            assert!(ids.contains(&item.as_str().unwrap()), "{item} is not a Models toolbar button");
        }
    }
}

#[test]
fn eloquent_model_class_line_gets_hub_gutter() {
    let source = "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nclass Post extends Model {}\n";
    let path = "/project/app/Models/Post.php";
    let mut index = Index::default();
    crate::eloquent_members::cache(&mut index.eloquent_models, source, path);
    let doc = Document { path: path.into(), language: "php".into(), version: 0, text: source.into() };

    let gutter = crate::model_gutter(&index, &doc).expect("model gutter");

    assert_eq!(gutter.line, 3);
    assert_eq!(gutter.command, "reveal");
}

#[test]
fn invokable_controller_gets_route_marker() {
    let host = MockHost::install();
    let plugin = &mut activated_with(&host, &[
        ("app/Http/Controllers/WishController.php", "<?php\nnamespace App\\Http\\Controllers;\n/**\n * Adds a family's item.\n */\nclass WishController\n{\n    public function __invoke()\n    {\n    }\n}\n"),
        ("routes/web.php", "<?php\nuse App\\Http\\Controllers\\WishController;\nRoute::middleware(['auth'])->prefix('x')->group(function () {\n    Route::post('/wish/{item}', WishController::class)->whereNumber('item')->name('wish');\n});\n"),
    ]);
    let text = host_text(&host, "app/Http/Controllers/WishController.php");

    let markers = plugin.line_markers(&doc("/project/app/Http/Controllers/WishController.php", "php", &text));

    assert!(markers.iter().any(|m| m.line == 7 && m.tooltip.starts_with("Route POST /x/wish")), "{markers:?}");
}

#[test]
fn controller_method_routes_share_one_native_chooser_icon_instead_of_a_route_actions_gutter() {
    use openidea_plugin::language_features::LanguageFeatures;
    let routes = "<?php\nuse App\\Http\\Controllers\\OrderController;\nRoute::get('/orders', [OrderController::class, 'index'])->name('orders.index');\nRoute::get('/archive', [OrderController::class, 'index']);\nRoute::post('/orders', [OrderController::class, 'store']);\n";
    let controller = "<?php\nnamespace App\\Http\\Controllers;\nclass OrderController\n{\n    public function index()\n    {\n    }\n    public function store()\n    {\n    }\n}\n";
    let host = MockHost::install();
    let plugin = &mut activated_with(&host, &[("app/Http/Controllers/OrderController.php", controller), ("routes/web.php", routes)]);
    let document = doc("/project/app/Http/Controllers/OrderController.php", "php", controller);

    let markers: Vec<_> = plugin.line_markers(&document).into_iter().filter(|m| m.icon.as_deref() == Some(crate::RELATED_MARKER)).collect();

    let targets = |line: u32| -> Vec<(u32, String)> {
        markers.iter().filter(|m| m.line == line).map(|m| {
            let target = m.target.as_ref().unwrap();
            assert!(target.path.ends_with("routes/web.php") && target.range.start.line == target.range.end.line && target.range.start.character < target.range.end.character, "{m:?}");
            let line = routes.lines().nth(target.range.start.line as usize).unwrap();
            (target.range.start.line, line[target.range.start.character as usize..target.range.end.character as usize].to_owned())
        }).collect()
    };
    assert_eq!(targets(4), [(2, "orders.index".to_owned()), (3, "get".to_owned())], "{markers:?}");
    assert_eq!(targets(7), [(4, "post".to_owned())], "{markers:?}");
    assert!(!plugin.gutter_actions(&document).iter().any(|g| g.command == "route-editor-action"));
}

#[test]
fn route_copy_curl_without_selection_explains() {
    let host = MockHost::install();
    let mut plugin = activated(&host);

    host.answer_next_prompt(None);
    assert_eq!(plugin.run_command("route-copy-curl", &host.invocation(), None).unwrap(), None, "palette run asks for a route; dismissing copies nothing");
    assert!(host.clipboard().is_none());
    host.answer_next_prompt(None);
    assert_eq!(plugin.run_command("model-open-source", &host.invocation(), None).unwrap(), None, "palette run asks for a model");
    assert_eq!(plugin.run_command("console-command-open", &host.invocation(), None).unwrap(), None, "palette run without a command class opens the Artisan tab");
    assert_eq!(host.revealed_views(), ["laravel"]);
}

#[test]
fn remove_phpdocs_command_rewrites_models_after_one_confirm() {
    let host = MockHost::install().with_capabilities(&["project.read", "project.write", "events", "process:php", "editor.read"]);
    host.add_file("app/Models/Tag.php", "<?php\n/**\n * @property int $id\n */\nclass Tag extends Model {}\n");
    let mut plugin = activated(&host);
    let project = host.invocation().project().unwrap();
    host.answer_next_prompt(None);
    plugin.run_command("remove-phpdocs", &host.invocation(), None).unwrap();
    assert!(project.read_file("app/Models/Tag.php").unwrap().contains("@property"), "declining writes nothing");
    host.answer_next_prompt(Some(&[]));
    plugin.run_command("remove-phpdocs", &host.invocation(), None).unwrap();
    assert_eq!(project.read_file("app/Models/Tag.php").unwrap(), "<?php\nclass Tag extends Model {}\n");
    assert!(host.notifications().iter().any(|n| n.body == "Removed IDE helper PHPDoc from 1 models."));
}

#[test]
fn regenerate_model_helpers_command_writes_the_picked_models() {
    let host = MockHost::install().with_capabilities(&["project.read", "project.write", "events", "process:php", "editor.read", "storage"]);
    host.add_file("artisan", "<?php");
    host.add_file("composer.json", r#"{"require": {"laravel/framework": "^13.0"},"autoload":{"psr-4":{"App\\":"app/"}}}"#);
    host.add_file("composer.lock", r#"{"packages":[{"name":"laravel/framework","version":"v13.40.0"}]}"#);
    host.add_file("app/Models/Post.php", "<?php namespace App\\Models;use Illuminate\\Database\\Eloquent\\Model;/** Handwritten Post documentation. */class Post extends Model {}");
    host.add_file("database/migrations/001.php", "<?php Schema::create('posts',function($t){$t->string('title');});");
    let mut plugin = Main::default();
    plugin.activate(&host.context("aritti.laravel")).unwrap();
    crate::tests::finish_indexing(&mut plugin);
    host.answer_next_prompt(Some(&["App\\Models\\Post"]));
    host.answer_next_prompt(Some(&["preserve"]));
    host.answer_next_prompt(Some(&[]));
    host.answer_next_prompt(Some(&["yes"]));
    host.answer_next_prompt(Some(&["yes"]));
    plugin.run_command("regenerate-model-helpers", &host.invocation(), None).unwrap();
    let source = host.invocation().project().unwrap().read_file("app/Models/Post.php").unwrap();
    assert!(source.contains("@property string $title") && source.contains("Handwritten"), "{source}");
    assert!(host.process_runs().is_empty());
}

#[test]
fn runtime_row_commands_from_the_palette_explain_what_to_read_first() {
    let host = MockHost::install().with_capabilities(&["project.read", "events", "process:php"]);
    let mut plugin = activated(&host);
    let error = plugin.run_command("runtime-retry-failed", &host.invocation(), None).unwrap_err();
    assert!(matches!(error, Error::Invalid(ref message) if message.starts_with("No failed jobs")), "{error:?}");
    let error = plugin.run_command("runtime-open-frame", &host.invocation(), None).unwrap_err();
    assert!(matches!(error, Error::Invalid(ref message) if message.starts_with("No errors")), "{error:?}");
    assert!(host.process_runs().is_empty());
}

#[test]
fn search_commands_say_indexing_is_not_finished() {
    let host = MockHost::install();
    laravel_project(&host);
    let mut plugin = Main::default();
    for id in ["search-names", "route-search"] {
        let error = plugin.run_command(id, &host.invocation(), None).unwrap_err();
        assert!(matches!(error, Error::Invalid(ref message) if message == "Laravel is still indexing. Try again in a moment."), "{id}: {error:?}");
    }
}

#[test]
fn generator_prepare_writes_the_files_after_one_confirm() {
    let host = MockHost::install().with_capabilities(&["project.read", "project.write", "events", "process:php", "editor.read", "storage"]);
    let mut plugin = activated(&host);
    host.add_dir("app");
    let project = host.invocation().project().unwrap();
    host.open_editor("/project/routes/web.php", "php", &project.read_file("routes/web.php").unwrap(), &[]);
    host.answer_next_prompt(Some(&["yes"]));
    let request = serde_json::json!({"op":"prepare","draft":{"name":"Invoice","table":"invoices","migrationStamp":"2026_11_10_120000","fields":[{"name":"title","kind":"string"}],"related":["migration"]}}).to_string();
    plugin.handle(generator_workspace::VIEW, ViewEvent::Message(request), &host.invocation()).unwrap();
    let reply = host.view_messages().last().unwrap().1.clone();
    assert!(reply.contains("Created or updated"), "{reply}");
    assert!(project.exists("app/Models/Invoice.php").unwrap());
}

#[test]
fn timeline_controller_method_gets_route_marker() {
    let host = MockHost::install();
    let plugin = &mut activated_with(&host, &[
        ("app/Http/Controllers/TimelineController.php", "<?php\n\nnamespace App\\Http\\Controllers;\n\nuse App\\Models\\Child;\nuse Illuminate\\Http\\Request;\n\nclass TimelineController extends Controller\n{\n    /** Days shown per page (the window ends on `end` and spans this many days). */\n    private const DaysPerPage = 1;\n\n    public function __construct(protected TimelineService $timeline, protected Clock $clock) {}\n\n    public function index(Request $request, Subjects $subjects): Response\n    {\n        $children = Child::orderBy('first_name')->get();\n        return response();\n    }\n}\n"),
        ("routes/web.php", "<?php\nuse App\\Http\\Controllers\\TimelineController;\nuse Illuminate\\Support\\Facades\\Route;\nRoute::middleware('auth')->group(function () {\n    Route::get('/timeline', [TimelineController::class, 'index'])->name('timeline');\n});\n"),
    ]);
    let text = host_text(&host, "app/Http/Controllers/TimelineController.php");

    let markers = plugin.line_markers(&doc("/project/app/Http/Controllers/TimelineController.php", "php", &text));

    assert!(markers.iter().any(|m| m.line == 14 && m.tooltip.starts_with("Route GET|HEAD /timeline")), "{markers:?}");
}

#[test]
fn class_names_resolve_against_each_text_in_turn() {
    let a = "<?php\nuse App\\Http\\Controllers\\TimelineController;\n";
    let b = "<?php\nuse Other\\TimelineController;\n";
    for _ in 0..2 {
        assert_eq!(crate::eloquent_members::source_class_name(a, "TimelineController").as_deref(), Some("App\\Http\\Controllers\\TimelineController"));
        assert_eq!(crate::eloquent_members::source_class_name(b, "TimelineController").as_deref(), Some("Other\\TimelineController"));
    }
}

#[test]
fn routes_search_tab_answers_from_the_finished_index_only() {
    use openidea_plugin::search::{SearchProvider, SearchTarget};
    let host = MockHost::install();
    laravel_project(&host);
    let mut plugin = Main::default();
    plugin.activate(&host.context("aritti.laravel")).unwrap();
    assert!(plugin.search("routes", "dashboard", 50, &host.invocation()).unwrap().items.is_empty(), "nothing while indexing");

    crate::tests::finish_indexing(&mut plugin);
    let page = plugin.search("routes", "dashboard", 50, &host.invocation()).unwrap();
    assert_eq!(page.items[0].title, "GET /admin/dashboard");
    assert!(matches!(&page.items[0].target, SearchTarget::Location(location) if location.path == "/project/routes/web.php" && location.range.start.line == 6));
    assert!(plugin.search("other", "dashboard", 50, &host.invocation()).unwrap().items.is_empty());
    assert!(host.process_runs().is_empty());
}

#[test]
fn version_status_item_opens_a_menu_of_declared_commands() {
    let host = MockHost::install();
    let mut plugin = activated(&host);
    plugin.indexes.get_mut("/project").unwrap().metadata.framework_version = Some((11, 2, 0));
    plugin.show_version_status("/project/routes/web.php");

    assert_eq!(host.status_items()["laravel-version"].text, "Laravel 11.2");
    let menu = host.status_menu("laravel-version");
    let commands: Vec<&str> = menu.iter().filter_map(|entry| match entry {
        openidea_plugin::StatusMenuEntry::Action { command, .. } => Some(command.as_str()),
        _ => None,
    }).collect();
    assert_eq!(commands, ["artisan-run", "migrate", "optimize-clear", "route-search", "route-send-request", "reveal", "reindex"]);
    assert!(commands.iter().all(|command| include_str!("../plugin.toml").contains(&format!("id = \"{command}\""))), "{commands:?}");
}
