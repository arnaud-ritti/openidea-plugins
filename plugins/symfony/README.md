# Symfony

Symfony support in PHP, Twig, YAML and XML files, built from your project's own sources. It completes and links routes, templates, services and translation keys, reports mistakes, and adds a Symfony tool window with routes, services, entities, the database and the profiler. MIT; see `LICENSE`.

## Requirements

- OpenIDEA with plugin API 1.14 or later (`engines.openidea = "^1.14"`).
- A Symfony project: it has `symfony.lock`, or `composer.json` requires `symfony/framework-bundle` or `symfony/symfony`.
- The `aritti.twig` plugin for Twig files. The IDE has no Twig language of its own.
- Optional: `php` on the `PATH` for cache commands, console commands, **Symfony: Make…**, the database commands and profile inspection.
- Optional: the `symfony` CLI for the server in the **Processes** tab and for **Symfony: New Symfony Project…** (Composer works too) and the **Symfony: Serve** run configuration.
- Optional: Docker, to run PHP (console commands, Send Request, the Database tab and run configurations) in a running container that mounts the project.
- Optional: a compiled dev cache (`var/cache/dev`) for missing-route and missing-service checks.

## Quick start

1. Install the plugin, and `aritti.twig` for Twig files ([how](../../README.md#install-a-plugin)).
2. Open your Symfony project. The status bar shows `Symfony 7.1` (your framework-bundle version) when it is recognised.
3. In a controller, type inside `$this->render('…')` or `redirectToRoute('…')` to complete names. ⌘B jumps to the template or route.
4. Open the **Symfony** tool window at the bottom and pick the **Routes** tab to browse routes.
5. Optional: allow `process:php`, then run **Symfony: Warm Up Cache** so bundle routes and services are known too.

## Features

### Complete and go to definition (⌘B)

Names are read from your sources and completed where Symfony uses them.

| Names | Read from | Completed in |
|---|---|---|
| Routes | `#[Route]` / `@Route` in `src/**/*Controller.php`, `config/routes*` YAML/XML, compiled `var/cache/dev/url_generating_routes.php` | PHP `generateUrl`, `redirectToRoute`, `->generate`; Twig `path()`, `url()` |
| Templates | `templates/**/*.twig` (`templates/bundles/XBundle/` as `@X/`; `twig: paths:` of `config/packages/twig.yaml` as `@namespace/`) | PHP `render*`, `stream`, `htmlTemplate`, `textTemplate`, `#[Template]`; Twig `extends`, `include`, `embed`, `import`, `from`, `use`, `include()`, `source()` |
| Blocks | the parent templates (`extends` chain) | Twig `{% block name %}`, `block('name')` |
| Variables | keys of the array controllers pass to `render()` | Twig expressions (completion only) |
| Services | `services:` in `config/**` YAML, `<service>` in XML, compiled `var/cache/dev/App_KernelDevDebugContainer.xml` | PHP `$container->get/has`, `service()`, `ref()`; YAML `'@id'`, `'@?id'`; XML `type="service" id=`, `alias=`, `parent=` |
| Classes | `composer.json` PSR-4 autoload directories | YAML service keys, `class:`, `parent:`; XML `class=` |
| Translations | `translations/**` YAML (nested keys joined with `.`) and XLIFF, by domain | PHP `->trans`, `t()`, `new TranslatableMessage`; Twig `'key'\|trans`; domain from the call, `{% trans_default_domain %}`, else `messages` |
| Assets | files under `public/` | Twig `asset()`; PHP `->getUrl()` |
| Entries | `webpack.config.js` `addEntry`/`addStyleEntry`, `importmap.php` | Twig `encore_entry_link_tags()`, `encore_entry_script_tags()`, `importmap()` |
| Roles | `ROLE_*` in security config and controllers, plus built-ins (`IS_AUTHENTICATED_*`, `PUBLIC_ACCESS`, …) | PHP `isGranted`, `denyAccessUnlessGranted`, `#[IsGranted]`; Twig `is_granted()` |
| Tags | Symfony's built-in service tags plus tags used in `config/**` YAML and XML | YAML `tags:` (`- { name: x }`, `- name: x`, `['x']`, `- x`); XML `<tag name="">` (completion only) |

Hover a route, service, template or translation key to see where it's declared, with a link to the Symfony docs. Built-in Twig functions, filters and tags (`path`, `asset`, `is_granted`, `form_*`, `|trans`, `{% trans %}`…), top-level bundle keys in `config/packages/*.yaml` (`framework:`, `doctrine:`, `security:`…), `%env()%` placeholders, roles, assets, entry points, Twig components and imported Symfony or Doctrine classes and attributes (`#[Route]`, `#[AsCommand]`, `AbstractType`, `#[ORM\Entity]`…) show a link to their Symfony docs page.

### Write Twig faster

- After `{%`: tag names (`if`, `for`, `block`, `extends`… and their `end…` forms).
- After `|`: filters. In expressions: functions. After `is` / `is not`: tests.
- Your own Twig extensions' filters, functions and tests are included (`getFilters()`, `#[AsTwigFilter]`, `#[AsTwigFunction]`, `#[AsTwigTest]`).
- After `alias.` of `{% import 'tpl' as alias %}` (or `_self`): that template's macros.
- Template paths after `extends`, `include`, `embed`, `import`, `from` and `use`.
- Variables: what controllers pass, what the template defines (`{% set %}`, `{% for %}` with `loop`, macro arguments, `{% with %}`, `{# @var name Type #}`), what `include` passes, `twig: globals:`, `app`, `_self` and `_context`.
- Members after `post.` when the type is known, including `post.author.` chains and loop variables.
- Forms: `form.` completes the fields your form type adds. ⌘B on a field jumps to its `->add()`.

### Complete members in PHP

- After `$variable->` or `$this->property->`, members are completed when the sources state the class.
- `$form->get('…')`, `has()` and `remove()` complete field names.
- The options array of `$builder->add('x', ChoiceType::class, ['…'])` completes option names.
- Inside `#[Route(`, `#[Template(`, `#[IsGranted(`, `#[AsCommand(`, `#[AsEventListener(` and Doctrine mapping attributes, argument names and some values complete. Invoke completion by hand there; the IDE doesn't pop it up.
- ⌘B on a member jumps to its property or method. Hover shows its inferred type.

### Front end: Stimulus, assets and UX components

- **Stimulus:** `data-controller="…"`, `stimulus_controller('…')`, `stimulus_action('…')` and `stimulus_target('…')` complete and open the controller file. Names come from `assets/controllers/**/*_controller.{js,ts}` (`users/list_item_controller.js` is `users--list-item`).
- **Assets:** `vite_entry_script_tags('…')` / `vite_entry_link_tags('…')` complete Vite entries. `importmap('…')` completes `importmap.php` entries. `asset('…')` also completes AssetMapper paths under `assets/`.
- **Twig types:** class names complete in `{% types name: '…' %}` and in `enum_cases('…')` / `enum('…')`.
- **Route parameters:** placeholders such as `{slug}` complete in the arguments of `path()`, `url()`, `generateUrl()`, `redirectToRoute()` and `->generate()`.
- **UX Twig components:** `<twig:Name>`, `</twig:Name>` and `component('Name')` complete and open the component. Attribute names and `<twig:block name="…">` complete too.

### Catch mistakes

Files are checked when you open or save them, unsaved changes included. Each group has its own switch in Settings.

- Missing templates, translation keys, routes and services. Routes and services need the compiled dev cache.
- Routes defined twice in a file, and XML services defined twice.
- Route controllers whose method doesn't exist, and deprecated route settings.
- Service problems: deprecated classes, duplicate parameters, unknown named arguments and `%parameters%`, missing classes, wrong argument count or types, missing tag interfaces.
- PHP calls: removed form type names, missing assets, unknown translation domains, deprecated classes.
- Twig: missing routes, assets and domains, `{% for … if %}`, component syntax mistakes, text outside blocks in a child template, hard-coded paths.
- YAML: unquoted `@`, `%` and other values Symfony's parser rejects.
- Console commands: missing `int` return types and invalid input modes.
- Deprecated Symfony APIs, gated on your `composer.json` version.
- Doctrine mapping and QueryBuilder problems.
- Practice hints, such as container access in controllers.

Silent cases: a name that is dynamic, comes from a library, or isn't in the index yet. The full rule list is in [Technical details](#technical-details).

### Fix with ⌥⏎

Quick fixes:

- Add a missing translation key to one of the domain's YAML files.
- Quote an unquoted `@` / `%` YAML value. Double an unescaped backslash.
- Fix a typo in a route, template, translation key or domain when exactly one known name is close.
- Replace a hard-coded path with `path()`.
- Switch `Routing\Annotation\Route` to `Routing\Attribute\Route`.

Refactorings (each edits the open file only):

- **Add #[Route] attribute** to a controller method.
- **Add parameter: Request $request** / **UserInterface $user** to a route action. In an `#[AsCommand]` `__invoke`: `InputInterface`, `OutputInterface`, `Cursor`, `SymfonyStyle`, `Application`.
- **Use TextType::class** for an old form type alias, or **Use class constants for all form types in this file**.
- **Override block "name"** from the `{% extends %}` line.
- **Use Product::class** for a Doctrine `'App:Product'` alias.
- **Convert to invokable command** (Symfony 7.3).
- **Convert Twig extension methods to attributes** (Twig 3.21).
- **Move $logger to the constructor**.
- **Extract translation** from selected text in a Twig template.
- **Add form fields for Product** in a form type with a `data_class`.
- **Extract to translation key…** on a selected string in Twig (`{{ 'Search' }}`) or PHP (`'Post saved'`). It asks for the key and shows the change; after you confirm, it adds the key to the default locale's `messages` file and replaces the string with `'key'|trans`, `$this->translator->trans('key')` or `t('key')`.
- **Inject Mailer instead of the container** on `$this->container->get(…)`, `$container->get(…)` or a controller's `$this->get('app.mailer')`. The service becomes a `private readonly` constructor parameter. In a controller action, **Inject Mailer as action argument** adds it to the action instead.
- **Add validation constraints to $title** on a Doctrine entity property, or **Add constraints to all fields of Post** on the class line: `#[Assert\NotBlank]` or `NotNull`, `Length(max: …)`, `Type(…)`, `Email` and a class `#[UniqueEntity]`, read from `#[ORM\Column]`.

### Rename

| What | Where to start | What changes |
| --- | --- | --- |
| Route | The editor's Rename on its name or a usage | The declaration and every usage. |
| Twig template | ⌥⏎ › **Rename or move template …** on a quoted template name, or anywhere in the template | Moves the file and updates `render()`, `renderView()`, `renderBlock()`, `#[Template]`, Twig `extends`, `include`, `embed`, `import`, `from`, `use`, `form_theme`, `include()`, `source()` and YAML `template:` keys. `@Namespace/` names keep their namespace. |
| Translation key | ⌥⏎ › **Rename translation key …** on a usage or on the key in a YAML or XLIFF file | One domain only: its YAML keys (nested or dotted), XLIFF `resname`/`id`/`<source>`, `\|trans`, `trans()`, `t()`, `TranslatableMessage` and form `'label' =>`. |
| Service id | ⌥⏎ › **Rename service …** on the id or a reference | Its definition in `services.yaml` or `.xml`, `@id`, `service('id')`, `!service_closure '@id'`, `alias:`, `<argument type="service" id>`, `alias="…"`, `#[Autowire(service: 'id')]` and `$container->get('id')`. Class-name ids are left to the PHP rename. |

You type the new name, review every changed line, then confirm. One undo reverts it all. If the name is also written somewhere the plugin can't prove is a plain reference (built at run time, in a comment, an array of fallbacks, an unknown domain), nothing changes and the message names that line.

### Create files with a review

These ⌥⏎ actions show the exact new file before creating it. Nothing is overwritten.

- **Create missing Twig template…** on a quoted name in `{% extends %}`, `include`, `embed` or `use`. Pick an empty file or a block scaffold. **Symfony: Create Missing Template or Class…** runs the same action from a command, or the editor context menu.
- **Symfony: Create Missing Template or Class…** on a class that doesn't exist yet (`new FooController`, `PostType::class`, a type hint, or `App\Controller\FooController::index` in a route or service file) runs the matching `make:*` command in a terminal after you confirm: `Controller`, `Command`, `Type` (form), `Voter`, `Subscriber`, `Listener`, `Message`/`Handler`, `Validator`/`Constraint`, `Fixtures` and `Test` classes, and entities under `App\Entity`. With nothing missing at the caret, it asks for a template name instead.
- **Add service definition to services.yaml** on a project class. It appends to your existing `config/services.yaml`, `.yml` or `.xml`.
- In service files: **Resync arguments with the constructor** (YAML), **Add arguments from the constructor** (XML) and **Add tags …**.
- In a PHP class or the root `composer.json`: a sibling service class or an empty compiler pass.
- From the root `composer.json`: a **Service Config (YAML/XML/PHP)** file under `config/`.
- **Generate a functional test for GET /blog…** on a routed controller method. It creates `tests/Controller/BlogControllerTest.php` (a `WebTestCase` with one test per HTTP method), or adds only the missing tests to that file when it exists.
- **Extract to Twig template…** on selected markup in a template. It asks for the name (`blog/_recent_posts.html.twig` by default), creates the template with the selection and replaces the selection with `{{ include('blog/_recent_posts.html.twig', {posts: posts}) }}`.
- **Create class PostInput…** on the type of a `#[MapRequestPayload]` or `#[MapQueryString]` parameter whose class doesn't exist: an empty `final class` where `composer.json` PSR-4 puts it. A name that isn't imported goes to `App\Dto` and gets a `use` line.
- **Extract request data to a DTO…** in a controller action that reads `$request->request->get('x')`, `$request->query->get('x')`, `$request->getPayload()->get('x')` or `$request->toArray()['x']`. It creates `App\Dto\CreateRequest` (named after the action) with one typed property per key, maps it with `#[MapRequestPayload] CreateRequest $input` (`#[MapQueryString]` for query reads), turns the reads into `$input->x` and drops `Request $request` when nothing else uses it. One confirm shows the new class and the changed action.

New › offers PHP class templates: **Symfony Command** (plain, invokable, with argument and option, with `#[Argument]` and `#[Option]`), **Symfony Controller** (attributes, annotations), **Symfony Form Type**, **Symfony Twig Extension** (methods, attributes), **Symfony Kernel Test**, **Symfony Web Test**, **Symfony Event Subscriber** and **Symfony Voter**. They use a fixed `App\…` namespace.

### Follow links in the gutter

A gutter icon on route usages (`generateUrl()`, `redirectToRoute()`, Twig `path()`/`url()`) goes to the route definition. Renaming a route updates its declaration and every usage.

A Symfony icon marks the classes and templates tied to other files. Click it to pick one from the **Choose Mapping Target** list; when there is only one, it opens right away. Hovering it shows *Related files*.

| Where | Related |
| --- | --- |
| A public method of a controller | The YAML routes whose `controller:` points to it and the templates it renders (`$this->render()`, `renderView()`, `#[Template]`) |
| The class line of an entity | Its `repositoryClass`, the classes of the same name with `Controller`, `Type`, `FormType`, `Fixtures` or `Repository` after it, the entities it has a relation to and the entities that have one to it |
| The class line of a controller, form type, fixture or repository | The entity of the same base name (`OrderController` → `Order`, or the entity whose `repositoryClass` it is) and the rest of that list |
| The first line of a Twig template | The controllers that render it |

The current file is read as you type; the other files come from the project index, updated on save.

### Use the Symfony tool window

The **Symfony** tool window at the bottom has these tabs:

- **About**: project, Symfony and key package versions, Composer requirements, whether the compiled cache was found, index counts and which `.env*` files exist (never their values). **Run bin/console about** opens it in a terminal.
- **Routes**: every route with method, path and controller. Filter to one controller, switch between flat and tree, and use **Copy as cURL**, **Open in Request Tab** or **Create HTTP Request File** on a row. Double-click opens the definition.
- **Entities**: a table of Doctrine entities from `#[ORM\Entity]` with table, field and relation counts, repository and file. Search also matches field names; **Show Details…** lists each field and relation.
- **Database**: the tables your entity mappings define, compared with the real database, plus a schema diagram and read-only rows. See [Inspect the database](#inspect-the-database).
- **Templates**: template name, namespace (filter), file and how many controller `render()` calls use it.
- **Components**: Twig components (`#[AsTwigComponent]`), Live components (`#[AsLiveComponent]`), anonymous components under `templates/components/` and Stimulus controllers, with the `<twig:Name>` tag, kind (filter), class, template, props and file. Double-click opens the class, or the template of an anonymous component. Right-click a row for **Open Template**, **Open Class** and **Copy Tag**.
- **Services**: service id, class (or `@target` for aliases), kind and file; filter by kind or file.
- **Events**: event listeners and subscribers grouped by event, highest priority first: `#[AsEventListener]`, `getSubscribedEvents()`, `kernel.event_listener` / `kernel.event_subscriber` tags in service files, and Doctrine's `#[AsDoctrineListener]` / `#[AsEntityListener]`. Columns are event, listener (`Class::method`), priority, kind (filter) and file. **Flat** / **Tree** switches between a list and grouping by event. Double-click opens the method.
- **Translations**: key, domain (filter), value and file.
- **Config**: container parameter names and where they're defined. Values aren't read.
- **Env**: `.env` variable names and the files defining them. Values are never read.
- **Request**: sends one request to a route and shows the response. See [Send a request to a route](#send-a-request-to-a-route).
- **Profiler**: the latest requests from the profiler's CSV index. See below.
- **Twig Namespace Paths**: navigation hints for extra Twig directories.
- **Runtime**: the newest entries of `var/log/<env>.log` with errors' stack frames, and Messenger failed messages to retry or remove. See [Read logs and failed messages](#read-logs-and-failed-messages).
- **Processes**: start the Symfony CLI server, server log and Messenger or scheduler workers in terminal tabs.
- **Console**: every `bin/console` command, grouped by namespace (**General** for unprefixed ones, up to eight **Recent**), with a form for its arguments and options. See [Run console commands](#run-console-commands).
- **Generators**: the MakerBundle `make:*` commands with the same form. Without MakerBundle the tab says "MakerBundle is not installed. Run composer require --dev symfony/maker-bundle, then click Reindex." and shows **Reindex**.

Templates, Components, Services, Events, Translations, Config, Env, Entities and Runtime are tables: type in the search box, narrow with the filters, sort by clicking a column, and **Reset filters** when nothing matches. Double-click or Enter opens the definition; right-click a row for its actions.

Tabs are grouped into four sections: **Browse** (About, Routes, Templates, Components, Services, Events, Config, Env, Translations, Twig Namespace Paths), **Data** (Entities, Database), **Run** (Request, Console, Profiler, Runtime, Processes) and **Build** (Generators). Switch section with the **View group** picker; each section remembers its last tab.

The toolbar has **Reindex** and **Warm Up Cache**. A tab that fails to load shows **Retry**. A tab that needs a permission names it.

### Look at recent requests in the profiler

1. Run **Symfony: Show Profiler** or click **Refresh** in the **Profiler** tab.
2. The latest 100 requests from `var/cache/dev/profiler/index.csv` appear.
3. Open a row to jump to its line in the CSV.
4. With `process:php` allowed, select a request and click **Inspect Profile** to see its collectors and numeric metrics.

Rows don't refresh by themselves. When the file changes, the status line says *The profiler index changed*; click **Refresh** to reload. Use **Choose CSV Source…** or **Find Cached CSV Source** if your profiler index lives elsewhere.

### Read logs and failed messages

The **Runtime** tab opens on the log of the current environment: `var/log/<APP_ENV>.log`, with `APP_ENV` from `.env.local`, else `.env`, else `dev`. No PHP runs for the log; the file is read as it is.

- Each row has the level, channel, time and message. Filter by level or channel, or type in the search box. **Show Details…** on a row gives the full message, the exception chain, the stack frames, and the context and extra data with passwords, tokens, secrets, cookies and `Authorization` values hidden.
- **Log File…** picks another file in `var/log`. **Refresh** reads the file again.
- Rows of errors with stack frames open a frame with a double-click or **Open Stack Frame…**; **Symfony: Open Error Stack Frame…** asks for the error, then the frame. Frames in `vendor/` are left out unless you click **Show Vendor Frames**. With a Docker target, paths inside the container open the same file in the project.
- **Failed Messages** runs `bin/console messenger:failed:show` and lists the default failure transport's messages: id, class, when it failed and the error. Right-click a row for **Retry…** (`messenger:failed:retry <id> --force`) or **Remove…** (`messenger:failed:remove <id> --force`). Both read the message again and ask first, naming its class, transport and error; removing can't be undone, and a retry runs the handler right away. **Symfony: Show Failed Messages**, **Symfony: Retry Failed Message…** and **Symfony: Remove Failed Message…** do the same from the palette.
- Without a failure transport the tab says how to set `failure_transport` in `config/packages/messenger.yaml`.

### Run console commands

- **Symfony: Warm Up Cache** and **Symfony: Clear Cache** run `php bin/console cache:warmup` / `cache:clear`, then reindex.
- **Symfony: Show Console** opens the **Console** tab. Click **Load commands** once: it runs `bin/console list --format=json` and keeps the list for the project (and its Docker target). **Refresh commands** reads it again.
- Select a command and fill in its form. Required arguments are marked `*`; repeated values go one per line; an option's default shows as a hint. The preview shows the project and the exact command line that will run.
- **Run** captures the output with `--no-interaction --no-ansi`, one run at a time per project. **Cancel** stops it; **Copy Output**, **Rerun** and **Session history** (the last 20 runs) are below. A maker or cache command that succeeds reindexes the project.
- **Run in Terminal** opens the command in a terminal tab without `--no-interaction`, so it can ask questions.
- **Generators** works the same way for `make:*` commands. Use **Run in Terminal** to let the maker ask for what's missing, or **Run** once every required argument is filled in. After a run, the files the maker reports as `created:` open in the editor.
- **Symfony: Run Console Command…** and **Symfony: Make…** ask which command and open it in the **Console** or **Generators** tab. Before the commands are loaded, they ask for the arguments and run the command in a terminal instead.
- **Symfony: Run Command from Class…** in a console command class asks for arguments and runs that command in a terminal. In another PHP file it asks which command first.
- **Symfony: Find Unused Templates** lists Twig templates that no controller, template or config file names. Pick one to open it. *maybe used* means the project builds template names at run time, so check before deleting.
- **Symfony: New Symfony Project…** asks for a parent folder, a name and the type (web application or microservice/API), then runs `symfony new` in a terminal, or `composer create-project symfony/skeleton` without the `symfony` CLI. When it finishes, it offers to open the project.

### Run console commands in Docker

1. Start the project's containers (Docker Compose or DDEV).
2. Allow `process:docker` and `storage` for Symfony.
3. Run **Symfony: Choose Docker Target…** and pick the container. Only running containers that mount this project are listed.

From then on, cache commands, the **Console** and **Generators** tabs, **Run Console Command…**, **Make…**, the Messenger and scheduler workers and **Run bin/console about** run `php bin/console …` inside that container with `docker exec`. The **Processes** tab names the container. Before each run the plugin checks that it is still the same running container with the same mounts; if not, it refuses and asks you to choose again. **Symfony: Clear Docker Target** goes back to local PHP. The choice is per project. You can also set the container in Settings instead; the PHP run configurations then stay local.

### Run configurations

Projects with `bin/console` or `symfony.lock` get these run configurations in the Run tool window:

| Run configuration | Runs | Offered when |
|---|---|---|
| **Symfony: Serve** | `symfony serve --no-tls` | `process:symfony` is allowed and the `symfony` CLI is installed. |
| **Symfony: Serve (PHP built-in server)** | `php -S 127.0.0.1:8000 -t public` | `process:php` is allowed. |
| **Symfony: Test** | `php bin/phpunit`, else `php vendor/bin/phpunit`, with `--configuration` and the first of `phpunit.xml`, `phpunit.xml.dist`, `phpunit.dist.xml` | One of those scripts or files exists. Results go to the test tree. |
| **Symfony: Messenger Worker** | `php bin/console messenger:consume <transports> -vv` | The project has `bin/console`. |
| **Symfony: Scheduler Worker** | `php bin/console messenger:consume scheduler_default -vv` | `symfony/scheduler` is in `vendor/` or `composer.json`. |

The Messenger worker consumes the transports listed in `config/packages/messenger.yaml`, except the failure transport and `sync://` ones. Without that file it consumes `async`.

With a Docker target picked for this project (see above), every configuration except **Symfony: Serve** runs inside the container with `docker exec`, and the built-in server listens on `0.0.0.0:8000` so a published port can reach it. A container set only in Settings keeps them local, because its mounts are checked only when a command runs. The list follows the current target each time the Run tool window refreshes it.

### Send a request to a route

1. Right-click a row in the **Routes** tab and pick **Open in Request Tab**, click **Request…** above a controller method, or run **Symfony: Open Route in Request Tab…** and choose a route.
2. The **Request** tab opens with the route. Pick the method, fill in one field per path placeholder (`{id}`, `{page?1}`, `{id<\d+>}`; the label shows its requirement and default), and add a query, a JSON body or a user to act as.
3. Click **Send Request**. The status, time, size, redirect, response headers and body appear below the form. While it runs, the form is read-only and **Cancel** stops it.

No server runs and nothing goes over the network: the plugin boots `App\Kernel` in a `php` process (local, or the Docker target) the way `public/index.php` does, and hands it the request.

- **Roll back database changes** is on by default. The request runs inside a transaction on every Doctrine connection, each rolled back afterwards. If Doctrine is missing or a transaction does not open on every connection, nothing is sent and the result says why. Emails sent synchronously, HTTP calls, files and cache writes are not rolled back.
- A POST, PUT, PATCH or DELETE asks first. With roll back on, the confirmation says that emails, HTTP calls, Messenger messages and files the request makes are not rolled back. With roll back off, it says that database writes, emails and files are kept and that this can't be undone.
- **Act as user** takes an email or username. It is looked up through a public user provider, or a Doctrine entity that implements `UserInterface` (its repository's loader, or an `email`, `username` or `identifier` field), and set as the security token of the `main` firewall. If the firewall replaces it, the result says so.
- **Accept JSON** is on for paths under `/api`; then the body is sent as JSON, otherwise as form fields. Forms with CSRF protection reject a sent form, because Symfony checks each form's token; the result says so on a 4xx response.
- **Copy as cURL** copies the request as you filled it in, against `http://localhost:8000`.

### Inspect the database

The **Database** tab has four parts:

- **Tables**: each table with its columns (type, length, nullable), indexes and foreign keys, read from the Doctrine attributes of your entities (`#[ORM\Table]`, `#[ORM\Column]`, `#[ORM\Id]`, associations with `#[ORM\JoinColumn]` and `#[ORM\JoinTable]`, `#[ORM\Index]`, `#[ORM\UniqueConstraint]`). Names the mapping leaves out follow Doctrine's naming strategy from `config/packages/doctrine.yaml` (the Symfony recipe's `underscore_number_aware`: `publishedAt` → `published_at`, join column `author_id`, join table `post_tag`). Double-click a table or column to open the entity at that property. Entities the plugin can't read (annotations, XML or YAML mappings, unreadable text) are listed below the tree.
- **Compare with database**: click **Compare with database** to see pending migrations, tables only in the mappings, tables only in the database and missing columns or indexes. Filter by status.
- **Schema**: a diagram of the tables. Drag to pan, scroll or use the buttons to zoom, **Fit** to see everything. Solid lines are foreign keys, dashed lines are associations with no foreign key between the two tables (an inverse side, or a many-to-many through its join table).
- **Rows**: pick a table and read its rows, 10 to 100 per page. Columns named like secrets or personal data (password, token, email, phone, address, …) are masked.

Tables and Schema read source only. Compare and Rows boot `App\Kernel` in a `php` process (local, or the Docker target) and run read-only queries on the default Doctrine connection.

The tab's toolbar has **Compare with Database**, **Run Migrations…**, **Add Database to IDE…** and, under **More**, **Manage Database Connections…**, **Reset Database…** and **Load Fixtures…**:

- **Symfony: Run Migrations…** asks first, like **Reset Database…** (type `migrate`), then runs `bin/console doctrine:migrations:migrate --no-interaction`. Undo needs `doctrine:migrations:migrate prev`. It needs DoctrineMigrationsBundle.
- **Symfony: Reset Database…** asks first, naming the project, the database and host (without credentials) and `APP_ENV` as the running app sees them, and the Docker container when there is one. The plugin reads these through PHP, so environment variables from Docker, your shell or `.env.local.php` count; the `.env` files are shown too when they say something else. It then runs `doctrine:schema:drop --force --full-database` and the migrations (or `doctrine:schema:create` without DoctrineMigrationsBundle). In the `prod` environment, when the app's values differ from the `.env` files, or when they can't be read, you also type `reset`.
- **Symfony: Load Fixtures…** asks the same way (type `load`), then runs `doctrine:fixtures:load`, which purges the database first. It needs DoctrineFixturesBundle.
- **Symfony: Add Database to IDE…** reads the app's Doctrine connections, asks which one when there are several, shows what will be added and adds it to the IDE's **Database** tool window as **Symfony · <connection>**. The password is never copied: enter it when you connect. With a Docker target, a database container's host and port become its published port on this machine (MySQL and PostgreSQL). A SQLite file must exist inside the project.
- **Symfony: Manage Database Connections…** lists this project's added databases: **Connect…** opens the IDE's connection (it asks for the password), **Remove from the IDE…** removes it and its saved password after you confirm. The database itself is not changed.

### Route actions in the editor

Above each public controller method that a route points to (a `#[Route]` in the same file, or a YAML route's `controller:`), and above each route of a `config/routes*` YAML file, a line of links shows:

- `GET|POST /path`: opens the route definition (the attribute or the YAML key).
- **Request…**: opens the route in the **Request** tab.
- **Route Actions…**: asks for **Copy URL**, **Copy as cURL** or **Create HTTP Request File**. Each asks for the base URL (`http://localhost:8000` by default); cURL and the HTTP file also ask for the method when the route has several.

A method with several routes shows these links once per route; a route with the same methods and path as one already shown adds none.

Each link reads the route again from its file before it acts. If the route changed, it asks you to use the updated link. Turn the links off with **Show route actions above routed controller methods and YAML routes** in Settings.

### Jump project-wide

- **Symfony: Go to Route…**, **Symfony: Go to Service…**, **Symfony: Go to Console Command…** and **Symfony: Go to Template…**.
- **Symfony: Find Source Occurrences** lists the literal references to the route, service or template under the caret.
- Search Everywhere (⇧⇧) has **Symfony Routes** and **Symfony Services** tabs: type part of a path, route name, controller or service id. They list what the project defines, once it is indexed.

### Colours in Run consoles

While a Symfony project is open, Run and Debug consoles colour Monolog lines at error level and above in red and warnings in yellow (log files, `bin/console -v` and the Symfony CLI server log), PHP fatal errors and warnings, `[ERROR]`, `[CAUTION]`, `[WARNING]` and `[OK]` blocks, `In File.php line N:` exception headers and exception class names. Trace frames in `vendor/` are dimmed.

### Let AI agents use the project (MCP)

Nine read-only MCP tools return CSV from the cached index. None runs PHP.

- `routes`, `services`, `commands`, `templates`, `parameters` (names only), `entities` and `env` (names and files, never values). Each takes `query`, `offset` and `limit`.
- `template_usage`: controllers rendering a template and templates that extend, include, embed, import or use it.
- `profiler_requests`: token, method, redacted URL, status and time from the profiler index.

Four more tools work with the commands loaded in the **Console** tab:

| Tool | What it does | Runs PHP |
|---|---|---|
| `console_commands` | Returns the loaded commands with their arguments and options (no default values) and a token. | No |
| `console_execute` | Runs one loaded command with the given values, captured like **Run** in the **Console** tab. Needs `runner` (`php` or `docker`, matching the project), `command`, the `token`, a `requestId` and `values`. Returns a job ID. You confirm the exact command line, the project, local PHP or the Docker container, and the `--env` first. Commands that print secrets (`secrets:reveal`, `secrets:decrypt-to-local`, `debug:dotenv`, `debug:container --env-vars`) are refused, and so are `messenger:consume`, `server:run`, `server:start`, `server:log` and `serve` without `--time-limit` or `--limit`. Refused in Restricted Mode. | Yes |
| `console_job` | Returns the job's status and output (up to 64 KiB), with URL credentials and password, secret, token, key and DSN values hidden. | No |
| `console_cancel` | Stops the job. | No |

### Icons and status bar

- `services.yaml`, `services.yml`, `routes.yaml`, `routes.yml`, `bundles.php` and `symfony.lock` get a small "S" icon. It's drawn for this plugin, not the Symfony logo.
- HTTP request files get per-method route icons.
- The status bar shows `Symfony 7.1` (the `symfony/framework-bundle` constraint, or just `Symfony`). The tooltip has route, service and template counts. Click it for a menu: Run Console Command…, Show Console, Clear Cache, Go to Route…, Go to Service…, Open Route in Request Tab…, Show Profiler and Reindex Project. It's hidden outside Symfony projects.

## Commands

Find them in Search Everywhere › Actions (⇧⇧) under **Symfony**; bind keys in Settings › Keymap. Commands show only in projects with `symfony.lock` or `bin/console`, except **New Symfony Project…**; the Twig ones only while a Twig file is focused, **Run Command from Class…** only while a PHP file is.

| Command | What it does | Shortcut |
|---|---|---|
| **Symfony: Reindex Project** | Rebuilds the index. | none |
| **Symfony: Warm Up Cache** | Runs `php bin/console cache:warmup`, then reindexes. | none |
| **Symfony: Clear Cache** | Runs `php bin/console cache:clear`, then reindexes. | none |
| **Symfony: Run Console Command…** | Picks a `bin/console` command and opens it in the **Console** tab. Before the commands are loaded there, asks for its arguments and runs it in a terminal. | none |
| **Symfony: Make…** | Picks a `make:*` command and opens it in the **Generators** tab. Before the commands are loaded there, asks for its inputs and runs it in a terminal. | none |
| **Symfony: Run Command from Class…** | Runs the console command of the open class in a terminal, with the arguments you type. In another PHP file, asks which command. | none |
| **Symfony: Find Unused Templates** | Lists Twig templates no code names; opens the one you pick. | none |
| **Symfony: New Symfony Project…** | Creates a web application or microservice/API project in a terminal, then offers to open it. | none |
| **Symfony: Choose Docker Target…** | Lists running containers that mount this project; console commands, **Send Request**, the **Database** tab and run configurations then run PHP in the one you pick. | none |
| **Symfony: Clear Docker Target** | Forgets the picked container; PHP runs locally again. | none |
| **Symfony: Go to Route…** | Jumps to a route definition. | ⌥⇧U (Alt+Shift+U) |
| **Symfony: Go to Service…** | Jumps to a service definition. | ⌥⇧S (Alt+Shift+S) |
| **Symfony: Go to Console Command…** | Jumps to a console command class. | none |
| **Symfony: Go to Template…** | Opens a Twig template. | none |
| **Symfony: Find Source Occurrences** | Lists usages in project sources of the route, service or template at the caret. | none |
| **Symfony: Extend Twig Template…** | Inserts an `extends` tag in the open template. Also in the editor context menu of Twig files. | none |
| **Symfony: Create Missing Template or Class…** | Creates the template directly after you confirm (not a single undo step). On a quoted reference it runs **Create missing Twig template…**; on a missing class it runs the matching `make:*` command in a terminal; otherwise, or without `editor.read`, it asks for a name and creates an empty `templates/<name>`. The name must end in `.twig` and cannot start with `/` or contain `..`, `\` or empty parts. Also in the editor context menu of PHP, Twig, YAML and XML files. | none |
| **Symfony: Insert Form Rows from a Form Type…** | Inserts `form_start`, one `form_row` per field and `form_end` at the caret. | none |
| **Symfony: Open Route in Request Tab…** | Opens the selected route row in the **Request** tab. With no row selected, asks which route. | none |
| **Symfony: Route Actions…** | Asks for **Copy URL**, **Copy as cURL** or **Create HTTP Request File**, then runs it on the route of the **Route Actions…** link, or on the selected route row. | none |
| **Symfony: Copy Route URL…** | Asks for the base URL, then copies it with the path of the selected route row. With no row selected, asks which route. | none |
| **Symfony: Copy Route as cURL…** | Asks for the base URL (and the method, when the route has several), then copies the selected route row as a `curl` command. With no row selected, asks which route. | none |
| **Symfony: Create HTTP Request File for Route…** | Asks for the base URL (and the method, when the route has several), then writes `http/<verb>-<path>.http` for the selected route row. With no row selected, asks which route. | none |
| **Symfony: Compare with Database** | Compares the database with the entity mappings and opens the **Database** tab. | none |
| **Symfony: Run Migrations…** | After a confirmation naming the database, runs `bin/console doctrine:migrations:migrate --no-interaction`. In `prod`, or when the app's settings differ from the `.env` files or can't be read, you also type `migrate`. | none |
| **Symfony: Reset Database…** | After a confirmation naming the database, drops every table and runs all migrations. In `prod`, or when the app's settings differ from the `.env` files or can't be read, you also type `reset`. | none |
| **Symfony: Load Fixtures…** | After a confirmation naming the database, purges it and runs `doctrine:fixtures:load`. In `prod`, or when the app's settings differ from the `.env` files or can't be read, you also type `load`. | none |
| **Symfony: Add Database to IDE…** | Adds a Doctrine connection to the IDE's **Database** tool window, without its password. | none |
| **Symfony: Manage Database Connections…** | Connects to, or removes, a database added with **Add Database to IDE…**. | none |
| **Symfony: Open Component Template…** | Opens the template of the selected **Components** row. With no row selected, asks which component. | none |
| **Symfony: Open Component Class…** | Opens the class of the selected **Components** row. With no row selected, asks which class component. | none |
| **Symfony: Copy Component Tag…** | Copies `<twig:Name />` for the selected **Components** row. With no row selected, asks which component. | none |
| **Symfony: Show Profiler** | Reads the profiler index and opens the **Profiler** tab. | none |
| **Symfony: Show Console** | Opens the **Console** tab. | none |
| **Symfony: Open Error Stack Frame…** | Opens a stack frame of the selected error in the **Runtime** tab. With no row selected, asks which error, then which frame. | none |
| **Symfony: Show Failed Messages** | Lists Messenger failed messages in the **Runtime** tab. | none |
| **Symfony: Retry Failed Message…** | Retries the selected failed message after you confirm. With no row selected, asks which message. | none |
| **Symfony: Remove Failed Message…** | Deletes the selected failed message from the failure transport after you confirm. With no row selected, asks which message. | none |
| **Symfony: Configure Twig Namespace Paths** | Opens the **Twig Namespace Paths** table. | none |

## Settings

Settings › Plugins › Symfony.

### Checks

Each switch is on by default.

| Setting | Key | Default |
|---|---|---|
| Report missing routes, templates, services and translation keys | `diagnostics` | on |
| Report route controllers without the named method and deprecated route settings | `controllerChecks` | on |
| Report deprecated service classes and service definition mistakes | `serviceChecks` | on |
| Report removed form type names, missing assets and domains in PHP | `phpCallChecks` | on |
| Report missing routes, assets and translation domains in Twig | `twigChecks` | on |
| Report deprecated for conditions and component syntax mistakes in Twig | `twigSyntaxChecks` | on |
| Report syntax problems in YAML configuration files | `yamlSyntaxChecks` | on |
| Report console command return types and input mode mistakes | `commandChecks` | on |
| Report deprecated Symfony APIs the project's version no longer supports | `deprecationChecks` | on |
| Hint at Symfony best practices and removed form and Twig APIs | `practiceChecks` | on |
| Report problems in Doctrine mappings | `doctrineChecks` | on |

### Editor

| Setting | Key | Default |
|---|---|---|
| Show route actions above routed controller methods and YAML routes | `routeLenses` | on |

### Project layout

| Setting | Key | Default | What it changes |
|---|---|---|---|
| Public directory that asset() paths start from (project-relative) | `publicDirectory` | `public` | Where asset completion and checks look. |
| Extra compiled container XML files (project-relative, comma-separated) | `compiledContainers` | empty | More compiled containers to read services and parameters from. |
| Extra compiled route files | `compiledRoutes` | empty | More compiled route files. |
| Extra service definition files outside config/ | `extraServiceFiles` | empty | More service files to index. |
| Extra routing files outside config/ | `extraRoutingFiles` | empty | More routing files to index. |
| Profiler index path | `profilerIndexPath` | `var/cache/dev/profiler/index.csv` | Which profiler index the **Profiler** tab reads. |
| Extra Twig namespaces (legacy) | `twigNamespaces` | empty | Extra Twig namespaces. Prefer **Symfony: Configure Twig Namespace Paths**. |

### Docker

A container picked with **Symfony: Choose Docker Target…** wins until you edit these settings.

| Setting | Key | Default | What it changes |
|---|---|---|---|
| Docker: socket address (unix:// path) | `dockerSocket` | `unix:///var/run/docker.sock` | Which local Docker daemon is used. |
| Docker: container ID that runs PHP | `dockerContainer` | empty | Full 64-character container ID. Empty: use the picked container, or local PHP. The PHP run configurations stay local with this setting alone. |
| Docker: project folder inside the container (absolute path) | `dockerWorkdir` | empty | Where the project is mounted in the container. Set it with the container ID. |

## Permissions

Optional permissions are off until you allow them in Settings › Plugins › Symfony.

| Permission | Used for | Without it |
|---|---|---|
| `process:php` | **Symfony: Warm Up Cache**, **Symfony: Clear Cache**, **Symfony: Run Console Command…**, **Symfony: Run Command from Class…**, **Symfony: Make…**, the **Console** and **Generators** tabs and `console_execute`, a missing class in **Symfony: Create Missing Template or Class…**, the database commands, Messenger/scheduler workers, the PHP run configurations, **Send Request**, **Compare with database** and **Rows** in the **Database** tab, **Inspect Profile**, and failed messages in the **Runtime** tab (**Symfony: Show Failed Messages**, **Symfony: Retry Failed Message…**, **Symfony: Remove Failed Message…**). | Those fail with "Running PHP needs the process:php permission. Allow it in Settings › Plugins › Symfony, and make sure php is on your PATH." **Make…** falls back to a built-in list. |
| `process:php-win` | On Windows, background PHP runs through `php-win` with no console window. | Background PHP uses `php`. |
| `process:docker` | **Symfony: Choose Docker Target…**, running console commands, **Send Request** and the **Database** tab in the chosen container, the PHP run configurations in a container picked for the project, and finding a database container's published port for **Add Database to IDE…**. | Console commands run with local PHP; with a container chosen or set, they fail with the permission message and the PHP run configurations aren't offered. |
| `process:symfony` | `symfony serve` and `symfony server:log` in the **Processes** tab, `symfony new` for **Symfony: New Symfony Project…** and the **Symfony: Serve** run configuration. | Those entries fail; **New Symfony Project…** uses Composer and **Symfony: Serve** isn't offered. |
| `process:composer` | **Symfony: New Symfony Project…** without the `symfony` CLI. | Without the CLI either, the command says which permission to allow. |
| `project.write` | **Create HTTP Request File** and **Symfony: Create Missing Template or Class…**. | The request is copied instead; the template command says which permission to allow. ⌥⏎ still creates templates. |
| `clipboard` | **Copy as cURL**, **Copy URL**, **Copy Tag** in the **Components** tab, **Copy as cURL** in the **Request** tab and **Copy Output** in the **Console** and **Generators** tabs. | Nothing is copied; a notice names the permission. |
| `events` | Refreshing cross-file problems and the index when files are opened, saved or changed. | Cross-file problems don't refresh on open or save; use **Reindex**. Live checks on the open file still work. |
| `editor.read`, `editor.write` | **Symfony: Extend Twig Template…**, inserting form rows, and reading the caret for **Symfony: Create Missing Template or Class…** and **Symfony: Run Command from Class…**. | **Extend Twig Template…** and the form rows fail; **Create Missing Template or Class…** asks for a template name and **Run Command from Class…** asks which command. |
| `storage` | Saving **Twig Namespace Paths**, the container picked with **Symfony: Choose Docker Target…**, the names shown by **Symfony: Manage Database Connections…** and the command list of the **Console** tab. | The tab shows an error and can't save; no container can be picked (the Docker settings still work); **Manage Database Connections…** shows IDs instead of names; the command list is loaded again after the plugin restarts. |
| `database.sources` | **Symfony: Add Database to IDE…** and **Symfony: Manage Database Connections…**. | Those commands name the permission and add nothing. |

## Troubleshooting

See also the [repository guide](../../README.md#troubleshooting).

| Problem | What to do |
|---|---|
| The tool window says "This project is not a Symfony application." | Add `symfony.lock`, or require `symfony/framework-bundle` in `composer.json`. Then click **Reindex**. |
| "Symfony source settings changed. Reindex to apply them." | Click **Reindex**. |
| Bundle routes or services aren't completed, or missing-route checks never fire | Run **Symfony: Warm Up Cache** so `var/cache/dev` exists. |
| "Running PHP needs the process:php permission. Allow it in Settings › Plugins › Symfony, and make sure php is on your PATH." | Allow `process:php` for Symfony. Check that `php` is on the `PATH` the IDE sees. |
| "bin/console cache:warmup failed: …" | Read the last lines shown. Fix the error in your app, then run it again. |
| "No console commands found. Allow process:php in Settings › Plugins › Symfony or define a command in the project." | Allow `process:php`, or add a command class. |
| "Docker container … was restarted or its image or mounts changed since you chose it. …" or "… no longer matches this project …" | Run **Symfony: Choose Docker Target…** again, or **Symfony: Clear Docker Target** to use local PHP. |
| "No running container mounts this project. …" | Start the containers. The project folder must be bind-mounted (Compose) or be a DDEV project. Or set the container in Settings. |
| Twig files get no completion | Install the `aritti.twig` plugin. |
| Attribute completion doesn't appear | Press the completion shortcut inside the attribute. |
| A check doesn't fire | The name may be dynamic, from `vendor/`, or not indexed yet. Check the group switch in Settings. |
| **Copy Route as cURL…** or **Create HTTP Request File for Route…** says "No routes are indexed." | Click **Reindex**, or check that the project defines routes. |
| Symfony commands are missing from Search Everywhere | They show only when the project has `symfony.lock` or `bin/console`, and the Twig ones only in a Twig file. Open the folder that holds them. |
| "Put the caret on a route, service or template name written as a plain string. …" | Put the caret inside a quoted name, not a variable or concatenation. |
| "The open file is not a Twig template." | Focus a `.twig` file. |
| "This template already extends another." | The template already has an `extends` tag. |
| The **Profiler** tab says "Refresh to read this project's profiler index …" or "No profiler data yet" | Load a page in dev mode so the index exists, then click **Refresh**. |
| The **Profiler** status says "The profiler index changed" | Click **Refresh** to load the new requests. |
| The **Runtime** tab says "var/log/dev.log doesn't exist yet." | Load a page or run a command in that environment, or pick another file with **Log File…**, then click **Refresh**. |
| "… is over 16 MiB, more than the IDE lets plugins read." | Empty or rotate the log (for example a `rotating_file` Monolog handler), then click **Refresh**. |
| **Failed Messages** shows "No failure transport" | Set `framework.messenger.failure_transport` in `config/packages/messenger.yaml` and define that transport. |
| "None of this error's stack frames is a file in this project." | The paths come from another machine or container. If PHP runs in Docker, run **Symfony: Choose Docker Target…**. |
| "Cannot load project Twig hints: … Optional storage permission is required." | Allow `storage` for Symfony. |
| "Nothing was sent: Roll back database changes is on, but a transaction could not be started on every Doctrine connection …" | Install Doctrine and check that the database is reachable from where PHP runs, or turn **Roll back database changes** off to send the request for real. |
| "Nothing was sent: the app did not boot (…)" | Run `composer install`; the app needs `vendor/autoload.php` and an `App\Kernel` class. Fix the error shown, then send again. |
| **Compare with database** or **Rows** says "The app did not boot (…)" or "Reading the database failed (…)" | Run `composer install` and check that the database in `DATABASE_URL` is reachable from where PHP runs (start the containers, or choose the Docker target). |
| Tables are missing from the **Database** tab, or "… entity files could not be read …" | Only attribute mappings are read. Entities with annotations, XML or YAML mappings are listed and left out. |
| "DoctrineMigrationsBundle is not installed in this project. …" | Run `composer require doctrine/doctrine-migrations-bundle` (or `doctrine/doctrine-fixtures-bundle` for fixtures). |
| "Nothing was changed: you typed … instead of reset." | Run the command again and type the word shown. |
| "Port … of the database container is not published on this machine. …" | Publish the database port in `compose.yaml`, or add the database in the **Database** tool window yourself. |
| "The firewall replaced the Act as user token …" | The firewall authenticates from the session or a header. Send the request without **Act as user**, or use a real login. |
| "This route changed since the link was shown. Use the updated link above the method." | Use the links shown now above the method; they follow the current file. |
| A tab's status line says "Some files weren't indexed" | The project has more files, or larger ones, than the index reads in one pass. Click **Reindex**; see the limits below. |

## Limitations

These need PhpStorm's full PHP, Twig and YAML models, which the plugin API doesn't offer. Member types are inferred from text.

- No Doctrine DQL strings or QueryBuilder field completion. No serializer support. Form `data_class` and extension options aren't read.
- Inspections and intentions that need resolved PHP types aren't available.
- No refactorings beyond those listed. Find Usages and the editor's Rename cover routes only; templates, translation keys and service ids are renamed from ⌥⏎.
- No MCP tools for Twig extensions, Doctrine or forms.
- No per-request profiler panels (headers, bodies, DB, logger, Twig collectors), remote profiler or runtime execution.
- No Doctrine `JOIN ... WITH` to `ON` or QueryBuilder `clearParameter` rules: there is no public deprecation notice to build them from.
- No `@Template` docblock completion (legacy annotations).
- Run configurations have no debugger: the plugin API offers none for PHP.
- Stopping a run configuration in Docker stops the `docker exec` client; the process in the container may keep running until the container restarts.
- Twig runtime loader resolution (ordering, namespace types, imported configuration) isn't reproduced.
- `vendor/` isn't read: library classes, bundle components and vendor Twig extensions are unknown.

## Technical details

<details>
<summary>Check rules, file actions, limits, caching and safety checks</summary>

### Runtime tab limits

- The log is read from its last 256 KiB, at most 500 entries, newest first. A file over 16 MiB can't be read. Both the Monolog line format (`[datetime] channel.LEVEL: message {context} {extra}`) and JSON lines are understood; other formats show no rows.
- Stack frames come from the `exception` in the context: its origin and each previous exception's, plus the trace when stack traces are on or the JSON formatter is used. At most 64 frames and 8 exceptions per entry; call arguments are never shown. Context and extra are cut at 4 KiB, 8 levels and 100 items per level.
- Values are hidden under keys containing password, passwd, secret, token, authorization, api_key, apikey, api-key, cookie or credential, along with `key=value` / `key: value` assignments of those words and `user:password@` in URLs. Other secrets written into messages are not recognised.
- `messenger:failed:show` has no JSON output, so its table is read: the first 100 messages of the default failure transport. Other failure transports (`--transport`) aren't listed. Each run is a `php bin/console` process, through the Docker target when one is chosen.

### Matching and indexing

- Everything is matched textually: a call is recognised by its method name, not its class. Templates are looked up in `templates/` and the `twig: paths:` directories of `config/packages/twig.yaml`.
- Facts are cached per open project from bounded reads of the project's own sources; `vendor/` isn't read.
- **Components** lists component classes under a `Components` folder of `src/` (`src/**/Components/**/*.php`), templates under `templates/components/` and Stimulus controllers under `assets/`. Bundle components aren't listed.
- **Events** reads listeners from `src/**/*Listener.php`, `src/**/*Subscriber.php` and `EventListener/` or `EventSubscriber/` folders of `src/`, plus the classes that `kernel.event_listener` / `kernel.event_subscriber` tags name (first 400 tags). Event constants of `KernelEvents`, `ConsoleEvents`, `FormEvents`, `SecurityEvents` and `WorkflowEvents` show as their names (`kernel.request`), Doctrine's `Events::x` as `x`, and `Foo::class` as the class name. A listener without a method uses Symfony's default (`onKernelRequest`, else `__invoke`); Doctrine listeners use the event name. Only literal priorities are read (others show 0), and `vendor/` listeners aren't listed.
- Saving a PHP file under `src/`, a template or a translation file updates only that file in the index. Adding or removing such a file, or changing `config/`, `composer.json`, `webpack.config.js`, `importmap.php` or the compiled cache files, rebuilds it.
- Templates under gitignored directories (for example `vendor/acme/bundle/templates` added through `twig: paths:`) are not listed, because the host skips ignored paths.
- File-system batches account for created, modified and removed paths. They rebuild an affected open project once per batch for conventional source/config roots or any enabled mapped subtree (including ancestor directory deletion; missing mapped directories stay watched so later creation can recover). Unrelated paths don't rebuild the index. Closed-project caches are pruned.
- Oversized batches (over 1,024 events/paths or a 4,096-byte path) drop cached facts rather than keep a truncated stale result. Reopening a source or explicit Reindex warms them again.
- Compiled-cache invalidation covers the four built-in route/container inputs read under `var/cache/dev` plus the configured `compiledRoutes`, `compiledContainers`, `extraServiceFiles` and `extraRoutingFiles` paths, including paths outside conventional source folders. Each comma-separated field is bounded to 32 KiB, 128 paths and 4,096 bytes per path. Absolute paths, empty/dot/traversal segments, backslashes, colons and control characters are ignored.
- When the current root's source settings have changed, cached facts are rejected and live diagnostics run on an empty partial index. Current-document checks continue; cached cross-file facts are unavailable until the next event batch or explicit Reindex (the main view offers Reindex).
- Cache admission marks glob errors, cap-sized results, required read failures and expired build budgets as partial. A cold nested root can't borrow a parent's cache.
- Hover and cached definition compare the selected cached root with the current host ownership of both the invoking document and the cached declaration. Cold nested projects, closed roots and newly foreign declarations stay quiet (metadata only, under the declared `project.read` grant).
- One shared UI-kit window holds the Symfony tabs, including Profiler and Twig Namespace Paths. Commands select their tab.

### Completion details

- Translation completion and lookup never fall back to another domain's catalogue when the requested domain has no indexed names.
- The host-index association graph for event listeners, Twig macros, voters, constraints, transformed class/template names and related gutter links is gated. Its global equality keys and synthetic anchors were navigation associations, not proven complete Rename symbols. Explicit Go to Definition from the table above and source-derived tool-window navigation remain; graph-only related gutter links and automatic usages/Rename are unavailable.
- *Related files* icons come from the `line-markers` provider, one marker per target on the same line, which the IDE shows as one icon. They publish no index keys, so Find Usages and Rename never see them. Routes declared with an attribute in the same file, functional tests, Doctrine migrations, form types matched by `data_class` and fixtures that only reference an entity aren't listed. Templates that extend, include or embed a template are left to the Twig plugin. The icon is the Symfony logo from `icon.svg` with the white tile removed.
- Twig tag, filter, function and test lists are static lists from Twig's public documentation plus the project's own extensions, read from the document index. Extensions in `vendor/` aren't read. Template paths come from the cached `templates/` index. Names starting with `@` are never reported missing.
- `IsGranted(attribute:)` offers the common role names and `AsEventListener(event:)` the core kernel and console event names. Attribute values are completed for `methods:`, `schemes:`, `type:`, `fetch:`, `cascade:`, `strategy:` and `onDelete:`. Doctrine attribute aliases are followed.
- A constraint argument such as `message: 'post.title.blank'` whose text looks like a key (a dot, no spaces) is a `validators` translation reference: completion, Go to Definition and the missing-key problem.
- Twig variable scopes: blocks, macros, loops and `with` have their own scope. Loop variables are scoped to their `{% for %}` body.
- Twig member types come from a `{# @var name Class #}` comment or what a controller passes to `render()`. PHP types come from a typed parameter or promoted property, `new Class`, `$repository->find()` / `findOneBy()` (the entity comes from `@extends ServiceEntityRepository<Entity>` or `parent::__construct($registry, Entity::class)`), `$em->find(Entity::class, …)`, `getRepository(Entity::class)->find()` and `$container->get(Class::class)`.
- Twig offers what `object.name` can read: public properties, public no-argument methods, and `getX`/`isX`/`hasX`/`canX` as `x`, following return and property types and project parent classes.
- Matching is textual and bounded: a reassigned or unknown value, a union type or a library class has no type and no members.
- Collections: `{% for post in posts %}` types `post` when `posts` comes from `findAll()` / `findBy()`, a `{# @var posts Post[] #}` comment, or a member whose docblock says `Post[]` or `Collection<int, Post>`. This works through `|slice`, `|reverse`, `|sort` and `|filter`.
- `$this->` inside a class offers its own members of every visibility and the public and protected ones of project parents. `$container->get('app.mailer')` follows the service id (and aliases) to the class declared in `config/**`.
- Query-builder chains such as `$repo->createQueryBuilder('p')->…->getQuery()->getResult()` give a list of the repository's entity (`getOneOrNullResult()` / `getSingleResult()` give one). `$em->createQueryBuilder()->select(…)->from(Post::class, 'p')` gives the entity named in `from()`.
- Hover on a typed member or variable shows its type, marked as inferred from sources.
- Forms: `$this->createForm(PostType::class)` (and `FormFactoryInterface::create()`) types the form. Passed to `render()` as `form` or `form.createView()`, Twig completes `form.` with the fields from `buildForm()`, `form.title.` with form view members and `form.vars.` with the usual variables, and follows embedded project form types (`form.address.street`). Built-in field type options come from Symfony's public documentation plus those every type has. Custom field types offer only the common options; `data_class`, `block_prefix` and extension options aren't read.
- Stimulus `data-controller` takes several space-separated names.
- Vite entries come from the `input` entries of `vite.config.{js,ts,mjs,mts}` and `assets/entrypoints/*`.
- Twig types complete without the `?` and `[]` decorations.
- Route placeholders such as `/posts/{slug}/{page<\d+>?1}` complete once the route's name is known from its definition.
- UX components come from `#[AsTwigComponent]` / `#[AsLiveComponent]` under `src/**/Components/` (with `name:` and `template:` arguments) or, for anonymous components, `templates/components/**`. Attribute names come from the class's public properties and `mount()` parameters plus `{% props %}`. Components of vendor bundles and ones registered by service tag aren't known.

### Check rules

Text inspections are silent when the class, service or file they need isn't in the index, and when a name is dynamic or comes from a library. Source-local rules (Twig syntax, YAML scalar, command, practice, deprecation and Doctrine/QueryBuilder checks, PHP route-class, asset, domain and form-alias checks, Twig cached checks, XML duplicate/factory/unknown-parameter and service-class checks) run on the current editor text, up to 32 KiB and at most 200 findings. They need an already recognised cached project. Cross-file checks read project sources on open/save.

- **Problems** (`diagnostics`): missing template, missing translation key (in a domain that has keys), missing route (once the compiled routes exist), missing service (once the compiled container exists), route defined twice in a file, service id defined twice in an XML file, unquoted YAML values starting with `@` or `%` in `config/` and `translations/` (Symfony's parser rejects them). Twig block tags never closed are reported by the `aritti.twig` plugin in Symfony projects.
- **Route controllers** (`controllerChecks`): `controller-method-missing`, a route in `config/routes*` (YAML/XML) naming `Class::method` where the project class has no such public method, an invokable class without `__invoke`, or a class-level `#[Route]` with neither `__invoke` nor routed methods. `deprecated-route-setting`, only these: the `Bundle:Controller:action` notation (removed in 4.0), `pattern` (removed in 3.0), `type: annotation` on imports (deprecated in 6.4; only reported when composer.json requires `symfony/framework-bundle` 6.4 or newer), and the `_method` / `_scheme` requirements (removed in 3.0; in XML, only in unprefixed routes/route/requirement structures). Classes extending a class other than `AbstractController`, using traits or defining `__call` aren't reported.
- **Services** (`serviceChecks`, YAML/XML under `config/`): `deprecated-service-class`, a `class:` naming a project class with `@deprecated` or `#[Deprecated]`. `duplicate-parameter-key`, one parameter twice in a file. `named-argument-missing`, a `$name` argument the class constructor has no parameter for (not for variadic constructors, services with `parent` or a factory, `calls`, or classes without a constructor; services with `constructor`, `parent` or a factory aren't checked for named arguments). `deprecated-factory-setting`, `factory_class` / `factory_method` / `factory_service` (YAML) or `factory-class` / `factory-method` / `factory-service` (XML), removed in Symfony 3.0, use `factory`. `unknown-parameter`, a `%name%` in `config/` YAML or XML that neither the config files nor the compiled container define (silent without a compiled container, for `%env(…)%`, `kernel.*`, `%%` and any other name shape). `service-class-missing`, a `class:` in one of the project's PSR-4 namespaces without a file (`Tests` namespaces skipped; complete index and strict literal production PSR-4 mappings required; based on the last project scan).
- **PHP calls** (`phpCallChecks`): `form-type-string`, `$builder->add('f', 'text')` with a core type alias removed in Symfony 3 (needs cached Composer evidence for Symfony 2.8 or newer). `missing-asset`, `->getUrl('x')` / `asset('x')` of a file absent from the public directory (not for URLs, absolute paths, dynamic names or a second package argument; assets under `build/`, `bundles/`, `assets/` and directories with no indexed file aren't reported; `->getUrl()` is only checked in files that mention `Packages` or `Symfony\Component\Asset`). `missing-translation-domain`, a domain with no file in `translations/` (only lower-case project-style names such as `messages`, `validators`, `security`; dotted or capitalised names such as `EasyAdminBundle` never are). `deprecated-class-usage`, `new X` / `X::class` of a project class marked `@deprecated`, from a file that isn't itself deprecated. Class files are read at most once per run (256 KB and 500 files caps); beyond that the checks stay silent. A parent named `Controller` / `AbstractController` counts only when it resolves to the framework's. A PHP scalar-literal argument needs exactly one quoted literal with an unescaped final closing quote; concatenation, ternaries/coalescing and adjacent literals are refused.
- **Twig** (`twigChecks`): `twig-missing-route`, `path('x')` / `url('x')` of a route absent from the compiled routes (silent without them, for `_`-prefixed routes and dynamic names). `twig-missing-asset`, with the same exceptions as in PHP (named asset packages are refused against the default public index). `twig-route-compare`, a literal compared with `app.current_route` or `app.request.attributes.get('_route')` (`==`, `!=`, `is same as`, `in [...]`) that isn't a compiled route. `twig-missing-translation-domain`. `twig-assets-tag-missing` (`{% javascripts %}` / `{% stylesheets %}` literals). `twig-enum-class`, `enum()` / `enum_cases()` naming a missing project class or one that is no enum. `twig-content-outside-block` (error; text or `{{ }}` outside blocks in a template whose first tag is a literal `{% extends %}`; `set`, `import`, `use`, `macro` and tags in general stay legal). `twig-path-replacement` (hint with a quick fix, `href="/blog"` / `action="/blog"` equal to the path of one YAML or XML route without placeholders). Template absence needs an observed bounded scan, unambiguous known paths and a static plain relative literal (concat/interpolation/unknown aliases are quiet). Key absence needs a known static domain (Info severity, locale/runtime caveats). Static ignore-missing targets can still report source-file absence. These checks run on `.twig` files from their extension, so they work without the `aritti.twig` plugin. Comments and `{% verbatim %}` bodies are skipped.
- **Twig syntax** (`twigSyntaxChecks`): `twig-for-condition` (warning; the `if` condition of `{% for %}`, deprecated in Twig 2.10, removed in 3.0, use `|filter`). `twig-component-block-syntax` (error), a `{% block %}` inside a `<twig:Name>` element. `twig-component-self-import` (error), `_self` as the source of `{% from %}` / `{% import %}` inside a component. An unbalanced `<twig:...>` file is left alone.
- **YAML syntax** (`yamlSyntaxChecks`, files under `config/` and `translations/`): `unquoted-yaml-scalar` (also a value starting with a backtick, or with `|` or `>` that is no block scalar header). `yaml-unescaped-backslash` (hint), a backslash in a double-quoted string that starts no escape. `yaml-unquoted-colon` (hint), a `: ` inside an unquoted value.
- **Commands** (`commandChecks`): `command-invokable-return-type` (hint), the `__invoke()` of a class with `#[AsCommand]` (the attribute must resolve to the Symfony class) has no `int` or `never` return type. `console-input-mode` (warning), two base modes ORed in the mode of `InputArgument` (`REQUIRED`, `OPTIONAL`) or `InputOption` (`VALUE_NONE`, `VALUE_REQUIRED`, `VALUE_OPTIONAL`), through `new` or `addArgument()` / `addOption()`; deprecated in Symfony 8.1. Modifiers, numbers and variables are ignored.
- **Deprecations** (`deprecationChecks`, PHP, each gated on `composer.json` requiring `symfony/framework-bundle` at the given version or newer): `filesystem-mirror-copy-on-windows` (8.1, the `copy_on_windows` option of `->mirror()` in a file mentioning `Symfony\Component\Filesystem`, use `follow_symlinks`). `tagged-collection-default-method` (8.1, `defaultIndexMethod:` / `defaultPriorityMethod:` with a string value on `TaggedIterator`, `TaggedLocator`, `AutowireIterator` or `AutowireLocator`, use `#[AsTaggedItem]`). `from-callable-option` (8.1, `alias`, `parent`, `synthetic`, `file`, `arguments`, `properties`, `configurator` or `calls` next to `from_callable` in one PHP-array service; YAML isn't checked). `http-kernel-event-argument` (4.3, `GetResponseEvent` family). `controller-get-doctrine` (5.4). `command-default-name` (6.1). `security-class-moved`, `request-get-content-type`, `first-class-callable` (`[$this, 'method']` of an own method), `render-form`, `form-create-view` (6.2). `route-attribute-namespace` (6.4, `Routing\Annotation\Route`, with a quick fix to `Routing\Attribute\Route`). `constraint-array-options` (7.4, `new NotBlank(['message' => …])` and the attribute form). Version gates use strict literal production `require` evidence from bounded valid JSON: a single numeric, caret, tilde, `>=` or trailing-wildcard requirement gives an advisory lower bound; unions, upper bounds, branches, stability modifiers, unsupported constraints and require-dev-only metadata stay quiet. Findings state that the installed/runtime version is unknown.
- **Doctrine** (`doctrineChecks`, PHP attributes, `*.orm.xml`, `*.orm.yml|yaml`; written from the Doctrine ORM documentation; PHP files count only when they import `Doctrine\ORM\Mapping` or a member of it, aliases followed): `orm-empty-table` (hint), `#[Table]` with no arguments or `name: ''`. `orm-table-deprecated-property` (warning), `indexes:` or `uniqueConstraints:` in `#[Table(...)]` (deprecated since ORM 3.2, use `#[Index]` / `#[UniqueConstraint]`). `orm-lifecycle-callbacks-missing` (warning), a lifecycle attribute (`PrePersist`, `PostPersist`, `PreUpdate`, `PostUpdate`, `PreRemove`, `PostRemove`, `PostLoad`, `PreFlush`) in an `#[Entity]` class without `#[HasLifecycleCallbacks]` (a `#[MappedSuperclass]` is skipped: a subclass may carry it). `orm-one-to-many-mapped-by` (warning), `#[OneToMany]` without `mappedBy` and without a `#[JoinTable]` on the property. `orm-inverse-side-join-column` (hint), `#[JoinColumn]` next to `#[OneToOne(mappedBy: ...)]`. `orm-join-column-nullable` (hint), an explicit `nullable: true` (the default) on a join column in PHP, XML (`<join-column>`) or YAML (`joinColumn` / `joinColumns`). `orm-lifecycle-method-visibility`, a lifecycle callback that isn't public. `orm-discriminator-map-duplicate` (PHP attribute and XML). `orm-query-iterate`, `orm-entity-manager-merge`, `orm-flush-entity` (removed in ORM 3, on conventionally named entity managers in files that import one). `orm-column-deprecated-type` (`array`, `object`). `collections-criteria-ordering` (hint, string orderings of `Criteria`). QueryBuilder checks run in files that import a Doctrine query builder or call `createQueryBuilder(`, with a `createQueryBuilder()` chain receiver or `$qb` / `$queryBuilder` / `$builder`: `query-builder-add`, `query-builder-array`, `query-builder-set-parameters` (hints), `query-builder-alias` (`from()` without alias), `query-builder-sort-direction`, `query-builder-time-parameter` (`'CURRENT_DATE'` bound as a parameter) and, with a DBAL import, `query-builder-parts` / `query-builder-execute` (deprecated in DBAL 3.1).
- **Service wiring** (`serviceChecks`, `src/wiring.rs`; a small class model of constructor parameters, parent and interfaces, resolved through `use`): `service-argument-count` (more positional arguments than the constructor has parameters). `service-argument-type` (a `@service` whose project class can't be the typed project class or interface). `service-tag-interface` (`kernel.event_subscriber`, `twig.extension`, `form.type`, `validator.constraint_validator`, `security.voter`, `kernel.cache_warmer`, `doctrine.event_subscriber` on a class that lacks the interface). `listener-method-missing` (`kernel.event_listener` with a `method` the class lacks). `yaml-php-const-missing` (`!php/const` of a project class without that constant). Silent when a parent or interface is outside the project and could matter, and for traits, `__call`, `parent`, factories, decorators, aliases, named or nested arguments. PHP service files (`$services->set(Foo::class)->args([service(Bar::class)])->tag('x')`) get the argument and tag checks. `type="constant"` XML values get the constant check. `autowire-wrong-class` compares `#[Autowire(service: 'id')]` with the parameter type.
- **Practices** (`practiceChecks`, PHP, hints unless noted; roles come from the parent class, interfaces or class attributes): `controller-container-get`, `controller-superglobal`, `controller-debug-call`, `controller-base-class` (the removed `Controller` base class). `constructor-container-injection`. `twig-extension-get-name`. `twig-legacy-class` (warning, `Twig_Foo` names). `form-type-removed-method` and `form-removed-option` (warnings, the Symfony 3.0 form removals). `constructor-request-injection`, `constructor-session-injection`, `constructor-token-injection`, `constructor-user-injection` (request-scoped values in the constructor of a controller, command, subscriber, voter, form type, Twig extension or attribute-registered service). `deprecated-injection` (warning; removed Symfony and Doctrine Persistence types such as `UserPasswordEncoderInterface`, `EngineInterface`). `kernel-project-dir` (a `KernelInterface` used only for `getProjectDir()`). `twig-extension-heavy-constructor` and `command-heavy-constructor`. `request-stack-in-action`. `controller-shortcut` (`new RedirectResponse`, `JsonResponse`, `BinaryFileResponse`). `controller-missing-exit` (warning, `$this->createNotFoundException()` or `render()` as a statement without `throw` / `return`). `bundle-register-commands`. `command-return-value` (warning, bare `return;`, `null`, booleans or strings from `execute()` / `__invoke()` of a `Command`). `request-bag-write` (`$request->attributes = …` on a parameter typed `Request`). `violations-find-by-codes` (literal code). `form-type-instance` (warning, `->add('f', new TextType())`).

### Quick fixes

- The missing-translation-key fix counts existing parsed key paths, even when the value is empty/null or a nested branch.
- Typo fixes need a unique candidate within one or two edits.
- Quick fixes follow the check settings.

### Refactorings

Each edits the open file only, and isn't offered when a needed import would clash with another class of the same short name.

- **Add #[Route] attribute**: caret in a public, non-static, non-magic method without `#[Route]` of a class named `*Controller` or extending `AbstractController` / `Controller`, with `symfony/framework-bundle` 6.2 or newer. Path and name come from the class and method names (`/blog/show-post`, `app_blog_show_post`); the caret isn't moved to the path.
- **Add parameter**: in a public route action (`#[Route]`, `@Route`, or `__invoke` of a class with a `#[Route]`). One action per type (no chooser). A type already in the signature, by short or full name, isn't offered.
- **Use TextType::class**: on the second argument of `->add('field', 'text')` when it is a pre-3.0 form type alias. The file-wide variant needs two or more.
- **Override block "name"**: one action per parent block the file doesn't define. Appends a block calling `parent()` at the end of the file.
- **Use Product::class**: a Doctrine `'App:Product'` alias in `getRepository()`, `find()`, `getReference()` or `getClassMetadata()` whose `App\Entity\Product` is a project class.
- **Convert to invokable command**: a class with `#[AsCommand]` extending `Command`, with `execute(InputInterface $input, OutputInterface $output): int` and a `configure()` of only `addArgument()` / `addOption()` calls (modes `REQUIRED` / `OPTIONAL`, `VALUE_NONE` / `VALUE_REQUIRED`). `extends` and `configure()` go, `execute` becomes `__invoke` with `#[Argument]` / `#[Option]` parameters, and a lone `parent::__construct();` goes. Not offered for anything else (`setName`, defaults, lifecycle methods, `$this` use of inherited members).
- **Convert Twig extension methods to attributes**: `getFilters()`, `getFunctions()` and `getTests()` holding only `new TwigFilter('x', [$this, 'method'])` entries become `#[AsTwigFilter('x')]` (and `AsTwigFunction`, `AsTwigTest`) on the methods. Not offered with options, other callables or other extension methods.
- **Move $logger to the constructor**: caret on a typed, uninitialised class-typed property without attributes. It becomes a promoted parameter of the existing constructor (not offered when that constructor assigns the property) or of a new one.
- **Extract translation**: replaces the selection with `{{ 'blog.index.read_more'|trans }}` and appends the key to the domain's YAML file (`{% trans_default_domain %}`, else `messages`; `.en.` file first). It follows the same known default-domain guard as reference operations: omitted defaults and one literal prefix directive are supported; dynamic, scoped or duplicate defaults never select `messages`.
- **Add form fields for Product**: in a form type whose `configureOptions()` sets `data_class`, appends `$builder->add('field', Type::class);` for the entity properties the form lacks (`#[ORM\Column]` types or typed properties; ids and associations skipped).
- **Extract to translation key…**: a single-line quoted string without backslashes (or `$` / `#` in double quotes), selected with or without its quotes. In Twig it must sit inside `{{ }}` or `{% %}` and not already be followed by `|trans`; selected Twig text keeps **Extract translation**. In PHP it must not already be the argument of `trans()`, `t()` or `new TranslatableMessage()`. The suggested key is the template name or the controller name (without `Controller`) plus the first words, made unique in the domain. The key goes to the Twig file's default domain (`messages` in PHP) in `<domain>+intl-icu.<locale>.yaml` or `<domain>.<locale>.yaml`; the locale is `framework.default_locale` (else `en`). Without such a file, `<domain>.<locale>.yaml` is created in `framework.translator.default_path` (else `translations/`). XLIFF files aren't written. PHP uses `$this->translator` when the class has a `translator` property or promoted parameter, `$translator` when the method has that argument, else `t()` with `use function Symfony\Component\Translation\t;` added. The confirm shows the translation file, the added line and the replacement; Cancel changes nothing.
- **Inject … instead of the container**: the class comes from `Foo::class`, a class-name id, or the project's service definitions (aliases followed). The parameter is named after the class without `Interface` (`$logger`). An existing constructor gets the parameter appended, so `parent::__construct()` calls stay; a class without one gets a new constructor, except when it extends a class other than `AbstractController` (the action then asks you to add one). A property or argument of the same name and type is reused; a different type is refused. Not offered inside the constructor or a static method. `$this->get()` is recognised only in classes extending `AbstractController` / `Controller`.

### Validation constraints and request DTOs

- **Add validation constraints**: Doctrine attribute mappings only (`#[ORM\Entity]` with `#[ORM\Column]` properties); ids and associations are skipped. A field that isn't nullable and has no default (`options: ['default' => …]` or a property value other than `null`) gets `NotBlank` (`string`, `text`) or `NotNull`. `integer`, `smallint`, `bigint` get `Type('integer')`, `decimal` `Type('numeric')`, `boolean` `Type('bool')`, date and time types `Type(\DateTimeInterface::class)`. `length: N` gives `Length(max: N)`, `unique: true` a class `#[UniqueEntity(fields: ['x'])]`, a column named `email` or `*_email` `Email`. A constraint the property already has, under any alias, isn't added again. It uses your `Constraints` import alias, or adds `use Symfony\Component\Validator\Constraints as Assert;`; not offered when `Assert` names another class.
- **Create class…**: the class goes to the longest matching `autoload` PSR-4 namespace of `composer.json` (one folder per namespace). Not offered for a class the index knows; an existing file or unsaved buffer at the target is refused, nothing is overwritten. `declare(strict_types=1);` is copied from the controller.
- **Extract request data to a DTO…**: `get()`, `getString()`, `getInt()` and `getBoolean()` reads with a quoted key. Properties are `string`, or `int` / `bool` with a `(int)` / `(bool)` cast or `getInt()` / `getBoolean()`. A read with a default becomes `?type $x = null` and `($input->x ?? default)`; fields read without one become required, so a request without them gets a 422 response. Not offered for dynamic keys, keys that aren't PHP names, other bag methods (`all()`, `has()`…), reads of both the query string and the body, `$request` inside strings, an action that already uses `$input`, or an existing `App\Dto\<Action>Request`.

### Rename of templates, translation keys and service ids

- These are file actions that preview their edits and ask first, not index anchors: the host would write an unchecked new name into every anchor, a nested YAML key isn't one token, and an anchor can't move a file. Only routes publish rename anchors.
- The action needs the full cached index and an unchanged, saved document. It scans the project's own sources: `src/`, `tests/`, `config/` and the `composer.json` PSR-4 folders for PHP, `templates/` and the `twig: paths:` folders for Twig, `config/**/*.{yaml,yml,xml}` (plus `translations/**` for keys), skipping `vendor/`, `node_modules/`, `var/` and nested projects. Uses in other folders aren't checked or renamed. It runs as a background task, so large projects can take up to a minute. Every spelling of the name must be a proven reference: names with a dot, slash or `@` are checked everywhere, including comments; plain words only where quoted. Limits: 2,000 files per pattern, 32 MiB in total, 128 KiB per file that mentions the name.
- New names: templates are a relative `.twig` path inside the same root and namespace, into an existing folder, never under `templates/bundles/`. Keys and ids use letters, digits, `.`, `_` and `-`. A nested YAML key keeps its parent path. Existing names are refused.
- After the confirmation it checks the scanned files again (re-reading the edited files, any whose size or modification time changed, and any modified from the second before the scan started, so a quick edit that keeps both is still caught); if anything changed, nothing is applied. A translation domain with PHP, JSON or other translation files is refused.

### File creation that previews first

- **Generate a functional test**: for each route of the method (attributes in the open file, other routes from the index), one test per HTTP method (`GET` when none). `GET` asserts `assertResponseIsSuccessful()`; other methods send an empty body and assert `assertResponseStatusCodeSame()` with 302 (303 with `HTTP_SEE_OTHER`) when the action redirects, else 201, 204 or 200 from the action's code. Placeholders get their default, else `1` for ids, numbers and `\d` requirements, `en` for `_locale`, `html` for `_format` and `test` otherwise. The namespace and folder come from the `autoload-dev` PSR-4 entry ending in `Tests` (else the first, else `App\Tests` in `tests/`). It confirms the exact file or added methods and never overwrites.
- **Extract to Twig template…**: needs a template the index knows. Refused when the selection starts or ends inside a tag, or opens or closes a Twig block (`if`, `for`, `block`, `embed`, `macro`, `with`, `apply`, `set`…) or an HTML element without the other half. Passed variables are the roots of plain `{{ var }}` / `{{ var.prop }}` prints and `{% for x in var %}` sources, minus names the selection binds, `loop`, `app` and Twig globals. The new file holds the selection with the first line's indentation removed. It confirms the target and content and never overwrites.

File-creating actions are lazy file actions with exact resulting-file confirmation. They need the current recognised root and full cached index, matching source version/text and bounded source/target reads. They re-run duplicate, prototype and target checks after the review, write nothing while preparing or cancelling, and refuse oversized previews rather than truncate. Values and tags are source-based suggestions, not runtime truth. Compiler Pass and Service Config files are created through these actions instead of templates.

- **Create missing Twig template…**: no trailing expression or options allowed on the reference. It confirms the target path and content, then creates the file (never overwriting) and opens it. Target: a validated relative ASCII `.twig` name (at most 512 bytes, 255 per segment) under the project's `templates/`. Empty/dot/traversal segments, absolute or URL paths, backslashes, dynamic strings, namespace/bundle names and arrays are refused. Existing files/directories, dirty targets and non-directory parents are refused (host stat canonicalizes in-root symlink targets, so not every symlink is detected; sandbox root confinement applies). Missing parents are delegated to the host Create. The source scanner skips comments, complete print strings and raw/verbatim bodies (source limited to 128 KiB). Provider discovery uses positive cached membership only and checks the actual file system and dirty buffers. Configuration discovery is capped at 33 paths. Supported YAML is limited to 64 KiB and rechecked after prompts. An explicit `default_path` must resolve literally to this project's `templates/`. Alternate roots, Twig namespace settings, imported/flow/alias loader mappings, PHP/XML loader configuration and layered `config/**/twig.*` files are outside this subset. It infers no parent template or neighbouring block names, and covers no PHP render calls, include functions/options, macros/imports, namespace mapping, service-class or Kernel generation.
- **Add service definition to services.yaml**: no file is created. It adds named arguments for the constructor's parameters without a default (services for class types, empty literals for scalars) and, without `autoconfigure: true`, the tags the class implies. Not offered for a class already defined or covered by a `resource:` prototype. **Resync arguments with the constructor** handles named `$arg` entries only; it keeps existing values, adds new parameters and drops removed ones. In XML, argument service ids are the type itself, or the only service whose class is the type. Generation reads at most 32 owned supporting files of 32 KiB each.
- **PHP class creation**: a plain sibling service class or an empty `CompilerPassInterface` implementation, with a class-name input and exact full-file confirmation. It needs exactly one matching production Composer PSR-4 string mapping among at most 64 validated entries (Composer JSON ≤ 64 KiB, duplicate decoded keys rejected; directory arrays, fallback namespaces, ambiguous, absolute/traversal/dot and autoload-dev-only mappings unsupported). It checks the source class, file name, namespace and directory, cached class-name collisions and a vacant target, then does one atomic non-overwriting Create/Open. The compiler pass uses fully qualified framework names and infers no framework installation, services, tags, constructor arguments or Kernel registration. The service scaffold is deliberately empty.
- **Service Config (YAML/XML/PHP)**: a plain file name directly under `config/` and an explicit mode: defaults only, or loading a selected production PSR-4 mapping. For the latter, the existing owned source directory is verified, with autowire/autoconfigure, the selected namespace/resource path and that root's `Kernel.php` exclusion; paths are encoded per format and nothing is guessed. One atomic non-overwriting Create/Open after exact confirmation. LF/CRLF follows the source. No imports, invoking-source edits or Kernel registration are added. Whether the file is loaded and which runtime services result stay unknown.

### Twig namespace path hints

- The table holds namespace, relative directory and Enabled for project-owned editor hints (empty namespace = main), with Add/Edit/Remove and Save/Cancel.
- The form previews the absolute target and labels every row as plugin metadata, not effective runtime loader configuration. Saving changes only optional plugin storage. Records carry the full project root, schema version and bounded rows. A conflicting concurrent save stays unsaved with a message.
- Enabled paths must exist under the invoking root with every component a directory. Absolute/traversal/glob/parameter paths and empty segments are refused.
- Bounds: 32 rows, 64 ASCII namespace bytes (letters, digits, underscore), 512 path bytes, 16 path components, 255 bytes per component, 32 KiB stored JSON. Unknown schema fields, duplicate namespace/directory pairs and mismatched stored roots are refused rather than rewritten. Storage denial shows an explicit error.
- The legacy global `twigNamespaces` setting (comma-separated `name=directory` pairs, empty name = main namespace, added to `twig: paths:`) stays readable with the same checks and is separate from this table. Its changes need an explicit Reindex if no relevant file-system event occurs.
- Validated enabled hints add completion and definition navigation with a "may differ at runtime" origin. Scans are bounded to 128 files per mapping, 1,024 total and a 250 ms budget. Conflicting logical names are suppressed. Missing-template warnings are suppressed while hints or unknown hint configuration could change resolution. Loader ordering, namespace types, imported configuration and runtime loader truth aren't reproduced.

### Hover

- Hover shows a **cached declaration candidate**: positive index facts of the current project only, with no provider-time file reads, scans or network access. The deepest known cached project root is used, so a sibling or nested project can't borrow another project's facts.
- Missing names and ambiguous templates stay quiet. A cached declaration in the current edited file is suppressed rather than showing an older version. Partial indexes are marked. Cache age, runtime and type resolution are unknown.
- No translated text, parameter values or service secret values are shown. A single known route definition can show its declared path and methods; duplicates don't get a guessed unique route.
- Bounds: exact current-literal UTF-16 ranges, 128 KiB and 4,096 Twig code regions/literals.
- Quiet contexts: Twig/HTML comments, raw/verbatim contents, HTML text and quoted expression text; PHP comment/outer-string contents, heredoc/nowdoc/backtick bodies, dynamic/interpolated/escaped literals; unproven XML namespace-qualified ownership (XML hover needs a namespace-aware element model first). YAML service references are limited to quoted source shapes under `config/`.
- Docs links come from a fixed list of symfony.com/doc/current pages (Twig built-ins link their anchor in the Twig extensions reference); unknown names get no link. PHP classes are resolved through the file's `use` imports and matched by namespace. Config keys link only where a bundle configuration reference page exists, so `doctrine_migrations:` and keys under `when@…:` get none.
- Translation hover resolves the exact literal call domain, a single known template-level Twig default-domain directive, or the standard `messages` fallback only when a domain was omitted. Explicit dynamic/missing/duplicate domains, incomplete calls and lookalike filters stay quiet.
- PHP lexical masking excludes heredoc/nowdoc bodies (ASCII labels up to 128 bytes, quoted/plain headers, indented closes and CRLF) and backtick literals while keeping byte positions and line breaks. Unfinished or unsupported literal headers consume the remaining source conservatively. Doc-route annotations come only from lexical doc comments.

### Tool window

- **Routes**: the method icon, the path, and grey text with `[METHOD] Controller` after it. *Filter* narrows the list to one controller (the status line says which), *Clear* restores it, and *Flat/Tree* groups the rows by controller. **Create HTTP Request File** writes `http/<verb>-<path>.http` with the base URL you enter (`http://localhost:8000` by default); without `project.write` the request is copied. The toolbar cache is bound to its rendered root: actions verify the current root, the unchanged selected fact and membership in the current cached index, and never execute an HTTP request. There's no request panel beside the list and no gutter icon on the handler.
- Route usages get a route icon that links to the route definition; controllers, entities, their family and templates get the *Related files* icon.
- **About** runs no PHP. Versions come from `composer.lock`.
- **Env** reads `.env`, `.env.local`, `.env.dev`, `.env.test`, `.env.prod` and their `.local` variants. Values are never read into the index or shown.
- **Entities** reads `src/Entity/` and the `dir:` of `doctrine.orm.mappings`. Fields show Doctrine type, nullability and `id`; relations show their target, in the row's details. Activating a row opens the entity class; fields and relations have no rows of their own. Legacy annotations, XML and YAML mappings, embeddables and mapped superclasses aren't read.

### Profiler

- Rendering performs no profiler file reads, processes or requests. URL userinfo is removed. Query/fragment presence is shown without their contents. IP and profiler token stay visible only in the project-owned review.
- Rows don't execute URLs or look up serialized profiles automatically. Selected details label the target as a source index row, not a collector/profile page.
- Open/double-click/Enter rechecks the invocation root, current key membership, exact source-file owner, file/byte/parser bounds and a unique current record against its cached physical line and displayed request facts, then opens that index line. Unknown, foreign, ambiguous, moved or changed records refuse navigation. The editor shows the original project-owned CSV.
- Bounds: the index file is limited to 1 MiB (checked by metadata before and by returned size after reading), at most 10,000 physical records and 16 KiB per record. The latest 100 valid unambiguous main requests are kept. Strict single-line CSV (quoted commas and escaped quotes supported); multiline or malformed data refuses the refresh. Unsupported IDs and case-insensitive duplicate tokens are omitted.
- Snapshots are bound to the project root and selected source path in session memory. Changing either hides former rows until an explicit refresh. A failed refresh keeps only that pair's last good data, with a note that it comes from the previous Refresh (missing, denied, unreadable, malformed and oversized outcomes are distinct). Switching or losing roots clears the single cache slot. Refresh and Open also need the index file's exact current project owner. File-system events on the selected source show *The profiler index changed*; data is adopted only on explicit refresh.
- The source is `profilerIndexPath` (empty uses the default; a normalized project-relative path up to 4096 bytes / 32 components, never absolute/traversal/glob) or a project-owned override stored by **Choose CSV Source…**. Blank input removes the override. Discovery lists owned existing CSV files plus the bounded compiled-container files, uses file metadata only, deduplicates shared cache directories and always asks. Invalid values hide cached data and refuse Open, never silently falling back to another source.
- **Inspect Profile** uses the selected request, or asks for one. `process:php` runs `php -n` with a 64 MiB memory limit and a two-second execution limit, without bootstrapping the application or loading Composer. Raw and GZIP file-storage profiles are bounded to 1 MiB input and decoded data, a 32-level serialized-data limit and at most 128 collectors. Only collector names, types and a fixed allowlist of nonnegative numeric metrics enter the UI; headers, request payloads, SQL, messages and arbitrary strings are withheld. PHP 8+ and built-in zlib are needed for GZIP. Seven-column legacy and eight-column `virtual_type` CSV are accepted; only request profiles are shown. Missing or denied tools or malformed profiles leave inspection unavailable.
- Per-request profiler details run project PHP and aren't offered as an MCP tool.

### Commands

- Cache commands are for trusted projects only. They reindex afterwards because the compiled files hold the routes and services of bundles.
- **Make...** reads `make:*` commands from `bin/console list make`, or a built-in list.

### Console and Generators tabs

- Rendering a tab runs nothing. Only **Load commands** / **Refresh commands** runs `bin/console list --format=json --no-interaction --no-ansi` (up to 4 MiB of output, at most 1,024 visible commands and 128 arguments or options each; hidden commands are skipped). The list is kept per project root and runner (local PHP or the Docker target) for the session and in plugin storage.
- Each run checks the form against the loaded definition: unknown names, a value for a checkbox, several values for a single-value field and a filled argument after an empty one are refused. Values are passed as separate arguments with no shell; positional arguments follow `--`. At most 512 arguments and 64 KiB.
- A captured run keeps at most 64 KiB of standard output and 16 KiB of standard error and is stopped after five minutes, also while its tab is hidden. One run per project at a time; the session history holds 20 runs (128 KiB) without their output. A run request ID is accepted once.
- For a Docker target, **Cancel** stops the `docker exec` client; PHP in the container may keep running.
- Files reported as `created: <path>` are opened only for relative paths inside the project, at most five.
- The MCP tools use the same list, token check, single run per project and limits. `console_commands` leaves out default values. `console_execute` asks you to confirm the exact command line before it starts (the Console tab's own **Run** doesn't), and is hidden, like `console_cancel`, in Restricted Mode. `console_job` hides URL credentials and the values of keys whose name contains password, secret, token, key, auth or dsn.
- **Insert Form Rows from a Form Type...** picks a project form type and inserts a `form_row` per literal `->add('name')`.
- **Create Missing Template or Class…** only makes classes under `App\`. A class counts as missing when it is neither indexed nor at its default `src/` (or `tests/` for `App\Tests\`) path. The name passed to the maker is relative to the maker's namespace (`Admin\FooController` for `App\Controller\Admin\FooController`), or fully qualified (`\App\Other\FooController`) outside it. Commands get `app:<kebab-name>`, handlers and validators lose their suffix, and `make:test` asks for the type and name itself.
- **Run Command from Class…** reads `#[AsCommand]`, `$defaultName` and `->setName()` in the open file and takes the first name, without aliases. Arguments are split like a shell does, without running a shell.
- **Find Unused Templates** reads `templates/` Twig names from the index and the text of `src/**/*.php`, `templates/**/*.twig` and `config/**/*.{yaml,yml,xml,php}`. When a pattern matches more than 5,000 files, it lists nothing and says so, since templates named only in the unread files would look unused. A template is used when any of them contains its exact name. Never listed: `bundles/`, `email/`, `emails/`, `form/` and `components/` templates, `base.html.twig`, `error*` files, namespaced (`@…`) templates and, when names are built at run time, `_*` partials. A render call or Twig `include`/`extends`/`embed`/`use`/`import`/`from`/`form_theme` with a non-string name, or a string name that is concatenated or interpolated, marks every result *maybe used*; the string start also hides the templates it prefixes. Tests and vendor code aren't read.
- **New Symfony Project…** runs `symfony new <folder> [--webapp]` when `symfony version` succeeds, otherwise `composer create-project symfony/skeleton <folder>` followed by `composer require webapp --working-dir=<folder>` for a web application. The project name is letters, numbers, `-` and `_` (at most 80). The folder path is limited to about 100 characters because the IDE's terminal key holds it, so the next step still runs if the plugin was unloaded meanwhile. An existing non-empty folder is refused by the tool itself.
- **Find Source Occurrences** needs a known source declaration. It scans fresh project-owned PHP, Twig and config YAML text for admitted reference contexts (comments, raw blocks, escaped/dynamic literals and other reference families are excluded). It rechecks the original editor and chosen source before navigation and offers exact file/line/column navigation. It publishes no automatic Rename or edit graph. Bounds: 500 files, 128 KiB per file, 8 MiB total, three seconds and 512 occurrences. Skipped, unreadable, oversized or capped sources are labelled **Partial**. Results are physical source-pattern candidates, not a complete runtime usage set.

### Docker target

- Discovery runs `docker --host <socket> ps --no-trunc --filter status=running`, then one `docker inspect` of those containers (at most 128). It never starts, creates or runs anything in a container. Compose and DDEV labels only name the entries.
- A container is offered only when it is running (not paused or restarting) and the most specific mount over its project folder is a bind mount of exactly this project, or a DDEV project volume at `/var/www/html` whose `com.ddev.approot` is this project. Nested mounts under the folder must be binds of the matching project subfolder.
- The pick is saved per project root in plugin storage (at most 64 KiB) with the container ID, image digest, start time and mounts. No environment is read. Editing the Docker settings replaces it.
- Before each console run a fresh `docker inspect` must match the saved one (or, for a container set in Settings, pass the same mount check). Then `docker --host <socket> exec [-it] --workdir <folder> <id> php bin/console …` runs, with argv only and no shell. Only `unix://` sockets with normalized absolute paths and full lowercase container IDs are accepted.
- `php -r` scripts over the host's 4 KiB argument limit go over stdin (`docker exec -i`).
- **Inspect Profile** keeps running local `php`: it reads profile files by their host paths. `symfony serve` and `symfony server:log` stay local too.

### Send Request

- **Send Request** is the only action that runs project PHP from the Request tab; filling in the form runs nothing. It runs `php -r <helper>` through the same runner as console commands (local `php`, `php-win` on Windows, or `docker exec` after the same container check). The helper and its input go over stdin.
- The helper loads `vendor/autoload.php`, then `config/bootstrap.php` when present, else `.env` with `Dotenv::bootEnv`. It reads `APP_ENV` (default `dev`) and `APP_DEBUG`, boots `App\Kernel`, builds the request with `Request::create()` and calls `handle()` and `terminate()`. It works with Symfony 5.4 to 7.x and checks each class before using it.
- Limits: one run per click, stopped after 60 seconds (Cancel stops it sooner) or 1 MiB of output. A hidden tab doesn't check on its run, so the next command, tab update or file event stops one past 60 seconds; the tab shows why when it opens again. Body up to 16 KiB of JSON (object or array), query up to 4,096 characters without spaces, user identifier up to 254 characters on one line, path values up to 512 characters. The response body shown is cut at 200,000 bytes, headers at 64 of 2,000 characters each. Drafts are kept for up to 8 projects, in memory.
- `Set-Cookie`, `Cookie`, `Authorization`, `Proxy-Authorization`, `X-CSRF-Token` and `X-XSRF-Token` are never returned; cookies show their name only (`name=•••`).
- Roll back opens a transaction on every connection `getConnectionNames()` lists (at most 32), nests the app's own transactions inside them and rolls them all back at the end. If the app ends an outer transaction itself, the result warns that changes may have been kept.
- Act as user doesn't log in through the firewall and doesn't create a session; the firewall name is always `main`.

### Database tab

- The page is the Laravel plugin's database workspace (`plugins/laravel/web/database-workspace.js` and `.css`, copied by `scripts/sync-plugin-ui.py`) with Symfony wording.
- Tables come from the attribute mappings of the entity folders the index reads (`src/Entity` plus `dir` of `doctrine.orm.mappings`), at most 2,000 files and 32 MiB. They are read again after a PHP file or `doctrine.yaml` changes, or on each refresh without the `events` permission. Mapped superclasses, inheritance (single-table and joined), embeddables and composite keys through associations are not modelled. Indexes Doctrine adds on its own for foreign keys aren't listed, so they never show as missing. The naming strategy is `underscore_number_aware` without `doctrine.yaml`, the one named in it, else Doctrine's default.
- Pending migrations are the `migrations/**/Version*.php` files whose class is not among the executed versions, read through `doctrine.migrations.dependency_factory` when the container exposes it, else from the `doctrine_migration_versions` table. Without that table every migration is pending.
- The helper (`src/database_probe.php`) runs like Send Request's: `php -r` through the console runner, stopped after 60 seconds or 4 MiB of output (cancelling the **Symfony: Compare with Database** progress stops it sooner). Closing the tab or switching to another one stops a Compare or Rows run; one left behind is stopped at 60 seconds by the next command, tab update or file event. It uses DBAL's schema manager (`createSchemaManager()`, `getSchemaManager()` on DBAL 2) and reads at most 256 tables, 512 columns and 64 indexes per table. Rows run `SELECT` with quoted column names on a table the schema manager lists, ordered by the primary key, 10 to 100 rows a page and 128 columns; values are cut at 200 characters and binary values are not shown. Masking matches whole words of the column name, and adjacent word pairs such as `backupCodes`. Driver messages lose everything up to the last `@` of a URL's authority (`//user:p@ss@host` becomes `//•••@host`) before they are shown; so do Send Request's errors. Each run gets a random token that its reply line must start with, so output the app prints can't pass for the reply.
- **Add Database to IDE…** exports only each connection's name, driver, host, port, database name, user and SQLite path; never the password, URL or driver options. IDs are `symfony-` and a hash of the project and connection name, so adding again updates the entry. With a Docker target it inspects the app container, lists the containers of its Compose project and keeps the one running container on a shared network whose alias or address is the database host; the port must be published on a loopback or wildcard address, on one port only.
- Run Migrations…, Reset Database… and Load Fixtures… run `bin/console … --no-interaction` to the end (up to 60 seconds) and show the last five output lines when they fail. The toolbar buttons and the commands share one confirmation. Before it, the helper's read-only `sources` mode reads the kernel's environment and the default connection's host and database; the password is not read.

### Run configurations

- Listing runs no process. It reads the marker files, `phpunit.xml*`, `messenger.yaml` (or `.yml`) and `composer.json`, and the saved Docker target.
- So the container is not re-inspected before a run, unlike console commands: a gone container makes `docker exec` fail in the Run tool window. Docker runs use `docker --host <socket> exec --workdir <folder> <id> php …` without `-it`.
- Only a container picked with **Symfony: Choose Docker Target…** (its mounts checked then) moves the PHP configurations into Docker; the Docker settings alone keep them local. An unreadable saved pick hides the PHP configurations instead of running them locally.
- `messenger.yaml` is read by indentation, not as full YAML: only the first `transports:` block counts, at most eight names made of letters, digits, `_`, `-` and `.`. Anchors and inline maps are ignored.
- The IDE shows a configuration only while its program is allowed and found, so **Symfony: Serve** disappears without the `symfony` CLI. Both serve configurations show when both are available.

### Capabilities

`project.read` is required. Optional, and each degrades gracefully when denied: `project.write` (Create HTTP Request File, Create Missing Template or Class command), `process:php` (cache warmup/clear, console, make, missing classes, database commands, workers, PHP run configurations, Send Request, the Database tab's Compare and Rows, and profiler inspection), `process:php-win` (windowless background PHP on Windows), `process:docker` (Docker target), `process:symfony` (Processes tab server and log, New Symfony Project, Symfony: Serve run configuration), `process:composer` (New Symfony Project without the symfony CLI), `events` (cross-file refresh on open/save; source-local syntax uses the live provider), `editor.read`/`editor.write` (Extend Twig Template, and the caret for Create Missing Template or Class and Run Command from Class), `clipboard` (Copy as cURL), `storage` (path hints, Docker target, database connection names) and `database.sources` (Add Database to IDE, Manage Database Connections). Missing-template creation uses file actions that preview their edits and ask first, with no process grant.

</details>

## Development

    cargo test
    openidea-plugin build
    openidea-plugin link
