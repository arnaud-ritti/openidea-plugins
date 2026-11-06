//! Work on many files at once, and generators that write their own files: models picked in the
//! Project view, IDE helper PHPDoc removal for all models, and factory and resource bodies filled
//! from the migration columns. Writing needs the optional `process:tee` capability.

use std::path::Path;

use openidea_plugin::{Error, Invocation, NotificationLevel, Project, host};

use crate::index::{Index, Names};
use crate::unused;

const SOURCE: &str = "Laravel";

pub fn write_file(project: &Project, relative: &str, content: &str) -> Result<(), Error> {
    let root = project.root()?;
    let clean = !relative.is_empty() && !relative.starts_with('/') && Path::new(relative).components().all(|c| matches!(c, std::path::Component::Normal(_)));
    if !clean || relative.contains(['\n', '\0']) {
        return Err(Error::Invalid(format!("{relative:?} is not a path inside the project")));
    }

    let path = format!("{}/{relative}", root.trim_end_matches('/'));
    host::run_process("tee", &[path], Some(content)).and_then(|out| {
        if out.status == Some(0) { Ok(()) } else { Err(Error::Invalid(out.stderr)) }
    })
}

/// Model class names of the model files among the selected Project view entries (a folder selects the models below it).
pub fn selected_models(index: &Index, selected: &[String]) -> Vec<(String, String)> {
    index
        .model_files
        .keys()
        .filter(|path| selected.iter().any(|s| path.as_str() == s || path.starts_with(&format!("{}/", s.trim_end_matches('/')))))
        .filter_map(|path| {
            let model = Path::new(path).file_stem()?.to_str()?.to_owned();
            index.models.contains_key(&model).then(|| (model, path.clone()))
        })
        .collect()
}

pub fn remove_phpdocs(ctx: &Invocation, index: Option<&Index>) -> Result<Option<String>, Error> {
    let project = ctx.project().ok_or("No project is open.")?;
    let index = index.ok_or("This project is not a Laravel application.")?;
    let selected = ctx.selected_paths();
    let models: Vec<(String, String)> = if selected.is_empty() {
        index.model_files.keys().filter_map(|path| Some((Path::new(path).file_stem()?.to_str()?.to_owned(), path.clone()))).collect()
    } else {
        selected_models(index, &selected)
    };

    let root = project.root()?;
    let mut edits: Vec<(String, String)> = Vec::new();
    for (_, path) in &models {
        let Some(relative) = path.strip_prefix(&format!("{root}/")) else { continue };
        let Ok(text) = project.read_file(relative) else { continue };
        let cleaned = text.match_indices("class ").find_map(|(at, _)| {
            let (range, replacement) = unused::remove_model_phpdocs(&text, at)?;
            let mut out = text.clone();
            out.replace_range(range, &replacement);
            Some(out)
        });
        if let Some(out) = cleaned {
            edits.push((relative.to_owned(), out));
        }
    }

    if edits.is_empty() {
        host::notify(NotificationLevel::Info, SOURCE, "No model carries IDE helper PHPDoc.");
        return Ok(None);
    }
    if !host::confirm("Remove IDE helper PHPDoc", &format!("Remove the @property, @method and @mixin lines from {} models?", edits.len()))? {
        return Ok(None);
    }

    for (relative, content) in &edits {
        match write_file(&project, relative, content) {
            Err(Error::Denied(_)) => return Err("Allow running tee for Laravel in Settings › Plugins, or use the quick fix on each model.".into()),
            result => result?,
        }
    }
    host::notify(NotificationLevel::Info, SOURCE, &format!("Removed IDE helper PHPDoc from {} models.", edits.len()));
    Ok(None)
}

fn faker(column: &str, kind: &str) -> Option<String> {
    let by_name = match column {
        "id" | "created_at" | "updated_at" | "deleted_at" | "remember_token" => return None,
        "email" => Some("fake()->unique()->safeEmail()"),
        "name" => Some("fake()->name()"),
        "password" => Some("bcrypt('password')"),
        "slug" => Some("fake()->slug()"),
        "title" => Some("fake()->sentence()"),
        "phone" => Some("fake()->phoneNumber()"),
        "url" => Some("fake()->url()"),
        "email_verified_at" => Some("now()"),
        _ => None,
    };
    if let Some(value) = by_name {
        return Some(value.to_owned());
    }
    if column.ends_with("_id") {
        let model: String = column.trim_end_matches("_id").split('_').map(|w| w[..1].to_uppercase() + &w[1..]).collect();
        return Some(format!("{model}::factory()"));
    }

    let value = match kind {
        "string" | "char" => "fake()->word()",
        "text" | "tinyText" | "mediumText" | "longText" => "fake()->paragraph()",
        "integer" | "tinyInteger" | "smallInteger" | "mediumInteger" | "bigInteger" | "unsignedInteger" | "unsignedTinyInteger"
        | "unsignedSmallInteger" | "unsignedMediumInteger" | "unsignedBigInteger" => "fake()->numberBetween(1, 1000)",
        "float" | "double" | "decimal" | "unsignedDecimal" => "fake()->randomFloat(2, 1, 1000)",
        "boolean" => "fake()->boolean()",
        "date" => "fake()->date()",
        "dateTime" | "dateTimeTz" | "timestamp" | "timestampTz" => "fake()->dateTime()",
        "time" | "timeTz" => "fake()->time()",
        "uuid" => "fake()->uuid()",
        "ulid" => "(string) \\Illuminate\\Support\\Str::ulid()",
        "ipAddress" => "fake()->ipv4()",
        "json" | "jsonb" => "[]",
        _ => return None,
    };
    Some(value.to_owned())
}

fn columns_of<'a>(index: &'a Index, model: &str) -> Option<&'a Names> {
    index.columns.get(index.models.get(model)?)
}

pub fn factory_source(index: &Index, model: &str, name: &str) -> String {
    let rows: String = columns_of(index, model)
        .into_iter()
        .flatten()
        .filter_map(|(column, entry)| Some(format!("            '{column}' => {},\n", faker(column, entry.detail.as_deref()?)?)))
        .collect();
    let imports: String = columns_of(index, model)
        .into_iter()
        .flatten()
        .filter(|(column, _)| column.ends_with("_id") && column.len() > 3)
        .filter_map(|(column, _)| {
            let related: String = column.trim_end_matches("_id").split('_').map(|w| w[..1].to_uppercase() + &w[1..]).collect();
            index.models.contains_key(&related).then(|| format!("use App\\Models\\{related};\n"))
        })
        .collect();

    format!(
        "<?php\n\nnamespace Database\\Factories;\n\n{imports}use Illuminate\\Database\\Eloquent\\Factories\\Factory;\n\n/**\n * @extends Factory<\\App\\Models\\{model}>\n */\nclass {name} extends Factory\n{{\n    public function definition(): array\n    {{\n        return [\n{rows}        ];\n    }}\n}}\n"
    )
}

pub fn resource_source(index: &Index, model: &str, name: &str) -> String {
    let rows: String = columns_of(index, model)
        .into_iter()
        .flatten()
        .filter(|(column, _)| !matches!(column.as_str(), "password" | "remember_token"))
        .map(|(column, _)| format!("            '{column}' => $this->{column},\n"))
        .collect();

    format!(
        "<?php\n\nnamespace App\\Http\\Resources;\n\nuse Illuminate\\Http\\Request;\nuse Illuminate\\Http\\Resources\\Json\\JsonResource;\n\nclass {name} extends JsonResource\n{{\n    public function toArray(Request $request): array\n    {{\n        return [\n{rows}        ];\n    }}\n}}\n"
    )
}

/// Writes a factory or resource with the model's columns in it; `None` when the kind has no such body or the name is nested.
pub fn model_file(project: &Project, index: &Index, kind: &str, model: &str, name: &str) -> Result<Option<String>, Error> {
    let (directory, source) = match kind {
        "factory" => ("database/factories", factory_source(index, model, name)),
        "resource" => ("app/Http/Resources", resource_source(index, model, name)),
        _ => return Ok(None),
    };
    if name.contains(['/', '\\']) || columns_of(index, model).is_none_or(|columns| columns.is_empty()) {
        return Ok(None);
    }

    let relative = format!("{directory}/{name}.php");
    if project.read_file(&relative).is_ok() {
        return Err(Error::Invalid(format!("{relative} already exists.")));
    }

    match write_file(project, &relative, &source) {
        Ok(()) => {
            let root = project.root()?;
            host::open_location(&format!("{}/{relative}", root.trim_end_matches('/')), 0, 0)?;
            Ok(Some(relative))
        }
        Err(Error::Denied(_)) => Ok(None),
        Err(error) => Err(error),
    }
}
