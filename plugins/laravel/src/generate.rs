use openidea_plugin::{Error, Invocation, PickItem, host, text};

use crate::index::Index;
use crate::refs::{self, Kind};

const FALLBACK_COMMANDS: &[&str] = &[
    "about", "db:seed", "db:wipe", "down", "env", "event:list", "key:generate", "migrate", "migrate:fresh",
    "migrate:refresh", "migrate:rollback", "migrate:status", "model:show", "optimize", "queue:failed", "queue:restart",
    "queue:retry", "route:list", "schedule:list", "schedule:run", "storage:link", "tinker", "up", "vendor:publish",
];

struct Generator {
    id: &'static str,
    label: &'static str,
    flags: &'static [(&'static str, &'static str)],
    model_option: Option<&'static str>,
    suffix: &'static str,
}

const fn kind(
    id: &'static str,
    label: &'static str,
    flags: &'static [(&'static str, &'static str)],
    model_option: Option<&'static str>,
    suffix: &'static str,
) -> Generator {
    Generator { id, label, flags, model_option, suffix }
}

const KINDS: &[Generator] = &[
    kind("model", "Eloquent model", &[
        ("-m", "Migration"),
        ("-f", "Factory"),
        ("-s", "Seeder"),
        ("-c", "Controller"),
        ("--resource", "Resource controller"),
        ("--requests", "Form requests"),
        ("--policy", "Policy"),
        ("--pivot", "Pivot model"),
        ("--all", "All of the above"),
    ], None, ""),
    kind("controller", "Controller", &[
        ("--resource", "Resource"),
        ("--api", "API resource"),
        ("--invokable", "Invokable"),
        ("--singleton", "Singleton"),
        ("--requests", "Form requests"),
    ], Some("--model"), "Controller"),
    kind("policy", "Policy", &[], Some("--model"), "Policy"),
    kind("observer", "Observer", &[], Some("--model"), "Observer"),
    kind("factory", "Factory", &[], Some("--model"), "Factory"),
    kind("resource", "JSON resource", &[("--collection", "Collection")], None, "Resource"),
    kind("request", "Form request", &[], None, "Request"),
    kind("migration", "Migration", &[], None, ""),
    kind("seeder", "Seeder", &[], None, "Seeder"),
    kind("event", "Event", &[], None, ""),
    kind("listener", "Listener", &[("--queued", "Queued")], None, ""),
    kind("job", "Job", &[("--sync", "Synchronous")], None, ""),
    kind("mail", "Mailable", &[], None, ""),
    kind("notification", "Notification", &[], None, ""),
    kind("middleware", "Middleware", &[], None, ""),
    kind("command", "Console command", &[], None, ""),
    kind("component", "Blade component", &[("--inline", "Inline view")], None, ""),
    kind("view", "Blade view", &[], None, ""),
    kind("test", "Test", &[("--unit", "Unit"), ("--pest", "Pest")], None, "Test"),
    kind("rule", "Validation rule", &[], None, ""),
    kind("cast", "Cast", &[], None, ""),
    kind("scope", "Scope", &[], None, ""),
    kind("provider", "Service provider", &[], None, "ServiceProvider"),
    kind("enum", "Enum", &[], None, ""),
    kind("class", "Class", &[], None, ""),
    kind("interface", "Interface", &[], None, ""),
    kind("trait", "Trait", &[], None, ""),
    kind("exception", "Exception", &[], None, ""),
    kind("channel", "Broadcast channel", &[], None, ""),
    kind("livewire", "Livewire component", &[("--inline", "Inline view")], None, ""),
];

const MODEL_AWARE: &[&str] = &["policy", "observer", "factory", "resource", "controller"];

fn items<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<PickItem> {
    pairs.map(|(key, label)| PickItem::new(key, label)).collect()
}

fn find(id: &str) -> Option<&'static Generator> {
    KINDS.iter().find(|kind| kind.id == id)
}

/// Letters, digits, `_`, `.`, `/`, `\` and `-` only, not starting with a dash: safe as one `php` argument and as a class or view name.
pub fn valid_name(name: &str) -> bool {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '\\' | '-');
    let first = name.chars().next();

    !name.is_empty()
        && name.len() <= 200
        && name.chars().all(allowed)
        && first.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.contains("..")
}

fn valid_class(name: &str) -> bool {
    name.split(['/', '\\']).all(|segment| {
        let mut chars = segment.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn valid_for(kind: &Generator, name: &str) -> bool {
    if !valid_name(name) {
        return false;
    }

    match kind.id {
        "view" | "component" | "livewire" | "migration" => true,
        _ => valid_class(name),
    }
}

fn run(command: &str, args: Vec<String>) -> Result<(), Error> {
    let mut full = vec!["artisan".to_owned(), command.to_owned()];
    full.extend(args);

    match host::open_terminal(&format!("artisan {command}"), "php", &full) {
        Err(Error::Denied(_)) => Err("Allow the process:php capability in Settings › Plugins.".into()),
        result => result,
    }
}

pub fn run_artisan(ctx: &Invocation) -> Result<Option<String>, Error> {
    let project = ctx.project().ok_or("No project is open.")?;
    let listed = project
        .run_process("php", &["artisan".to_owned(), "list".to_owned(), "--raw".to_owned()], None)
        .ok()
        .filter(|output| output.status == Some(0))
        .map(|output| output.stdout);

    let commands: Vec<(String, String)> = match &listed {
        Some(stdout) => stdout
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let (name, description) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
                (!name.is_empty()).then(|| (name.to_owned(), description.trim().to_owned()))
            })
            .collect(),
        None => FALLBACK_COMMANDS.iter().map(|name| ((*name).to_owned(), String::new())).collect(),
    };

    let choices: Vec<PickItem> = commands
        .iter()
        .map(|(name, description)| PickItem::new(name, name).description(description))
        .collect();
    let Some(command) = host::pick("Artisan command", &choices)? else { return Ok(None) };
    if !commands.iter().any(|(name, _)| *name == command) {
        return Err(Error::Invalid(format!("unknown artisan command {command}")));
    }

    let Some(arguments) = host::input(&format!("Arguments for artisan {command} (separated by spaces)"), None)? else {
        return Ok(None);
    };
    let args: Vec<String> = arguments.split_whitespace().map(str::to_owned).collect();
    if let Some(bad) = args.iter().find(|arg| arg.chars().any(|c| c.is_control())) {
        return Err(Error::Invalid(format!("bad argument {bad:?}")));
    }

    run(&command, args)?;
    Ok(None)
}

fn pick_model(title: &str, index: Option<&Index>, optional: bool) -> Result<Option<Option<String>>, Error> {
    let mut choices = Vec::new();
    if optional {
        choices.push(PickItem::new("", "No model"));
    }
    if let Some(index) = index {
        choices.extend(items(index.models.keys().map(|model| (model.as_str(), model.as_str()))));
    }
    if choices.is_empty() || (optional && choices.len() == 1) {
        return Ok(Some(None));
    }

    let Some(chosen) = host::pick(title, &choices)? else { return Ok(None) };
    if chosen.is_empty() {
        return Ok(Some(None));
    }
    if !index.is_some_and(|index| index.models.contains_key(&chosen)) {
        return Err(Error::Invalid(format!("unknown model {chosen}")));
    }

    Ok(Some(Some(chosen)))
}

fn finish(ctx: &Invocation, kind: &Generator, name: &str, model: Option<String>, flags: Vec<String>) -> Result<Option<String>, Error> {
    ctx.project().ok_or("No project is open.")?;
    let mut args = vec![name.to_owned()];

    if let (Some(option), Some(model)) = (kind.model_option, model) {
        args.push(format!("{option}={model}"));
    }
    args.extend(flags);

    run(&format!("make:{}", kind.id), args)?;
    Ok(None)
}

/// `fixed` skips the kind pick (New Eloquent Model).
pub fn make(ctx: &Invocation, index: Option<&Index>, fixed: Option<&str>, has_livewire: bool) -> Result<Option<String>, Error> {
    let kind = match fixed {
        Some(id) => find(id).ok_or_else(|| Error::NotFound(format!("generator {id}")))?,
        None => {
            let choices = items(KINDS.iter().filter(|k| has_livewire || k.id != "livewire").map(|k| (k.id, k.label)));
            let Some(chosen) = host::pick("Generate with artisan make", &choices)? else { return Ok(None) };
            find(&chosen).ok_or_else(|| Error::Invalid(format!("unknown generator {chosen}")))?
        }
    };

    let Some(name) = host::input(&format!("{} name", kind.label), None)? else { return Ok(None) };
    let name = name.trim().to_owned();
    if !valid_for(kind, &name) {
        return Err(Error::Invalid(format!("{name:?} is not a valid {} name", kind.label.to_lowercase())));
    }

    let mut flags = Vec::new();
    if !kind.flags.is_empty() {
        let choices = items(kind.flags.iter().copied());
        let Some(chosen) = host::pick_many(&format!("Options for {}", kind.label.to_lowercase()), &choices)? else {
            return Ok(None);
        };
        if let Some(bad) = chosen.iter().find(|flag| !kind.flags.iter().any(|(known, _)| known == flag)) {
            return Err(Error::Invalid(format!("unknown option {bad}")));
        }
        flags = chosen;
    }

    let model = match kind.model_option {
        Some(_) => match pick_model("Bind to model", index, true)? {
            Some(model) => model,
            None => return Ok(None),
        },
        None => None,
    };

    finish(ctx, kind, &name, model, flags)
}

pub fn for_model(ctx: &Invocation, index: Option<&Index>) -> Result<Option<String>, Error> {
    let Some(Some(model)) = pick_model("Model", index, false)? else {
        return Err("No Eloquent model found in the project.".into());
    };

    let choices = items(MODEL_AWARE.iter().filter_map(|id| find(id)).map(|k| (k.id, k.label)));
    let Some(chosen) = host::pick(&format!("Generate for {model}"), &choices)? else { return Ok(None) };
    let kind = find(&chosen)
        .filter(|k| MODEL_AWARE.contains(&k.id))
        .ok_or_else(|| Error::Invalid(format!("unknown generator {chosen}")))?;

    let default = format!("{model}{}", kind.suffix);
    let Some(name) = host::input(&format!("{} name", kind.label), Some(&default))? else { return Ok(None) };
    let name = name.trim().to_owned();
    if !valid_for(kind, &name) {
        return Err(Error::Invalid(format!("{name:?} is not a valid {} name", kind.label.to_lowercase())));
    }

    let mut flags = Vec::new();
    if kind.id == "controller" {
        flags.push("--resource".to_owned());
    }

    finish(ctx, kind, &name, Some(model).filter(|_| kind.model_option.is_some()), flags)
}

fn studly(segment: &str) -> String {
    segment
        .split('-')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map(|c| c.to_ascii_uppercase().to_string() + chars.as_str()).unwrap_or_default()
        })
        .collect()
}

/// Creates the view, component, Livewire component or controller the name under the cursor points to.
pub fn create_missing(ctx: &Invocation, index: Option<&Index>) -> Result<Option<String>, Error> {
    let editor = ctx.editor().ok_or("Open a file first.")?;
    let document = editor.document()?;
    let cursor = editor.selections()?.first().map(|range| range.start).ok_or("No cursor.")?;
    let offset = text::offset(&document.text, cursor).ok_or("No cursor.")?;
    let language = crate::language_of(&editor.path(), &editor.language()).ok_or("Not a PHP or Blade file.")?;
    let index = index.ok_or("The project is not indexed.")?;

    let reference = refs::reference_at(language, &document.text, offset).ok_or("No Laravel name at the cursor.")?;
    let name = &document.text[reference.start..reference.end];

    let (id, argument) = match reference.kind {
        Kind::View if !index.views.contains_key(name) => ("view", name.to_owned()),
        Kind::Component if !index.components.contains_key(name) => {
            ("component", name.split('.').map(studly).collect::<Vec<_>>().join("/"))
        }
        Kind::Livewire if !index.livewire.contains_key(name) => ("livewire", name.to_owned()),
        Kind::Controller | Kind::Action => {
            let class = name.split('@').next().unwrap_or(name);
            let short = class.rsplit('\\').next().unwrap_or(class);
            if index.controllers.contains_key(short) {
                return Err("Nothing missing here.".into());
            }
            ("controller", class.to_owned())
        }
        _ => return Err("Nothing missing here.".into()),
    };

    let kind = find(id).ok_or_else(|| Error::NotFound(format!("generator {id}")))?;
    if !valid_for(kind, &argument) {
        return Err(Error::Invalid(format!("{argument:?} is not a valid {} name", kind.label.to_lowercase())));
    }
    if !host::confirm("Create missing target", &format!("Run php artisan make:{id} {argument}?"))? {
        return Ok(None);
    }

    finish(ctx, kind, &argument, None, Vec::new())
}
