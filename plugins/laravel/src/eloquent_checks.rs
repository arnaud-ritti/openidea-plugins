//! Eloquent checks that read the text of one file and, for models, the model's own file.

use std::ops::Range;

use openidea_plugin::Severity;

use crate::php;

pub struct Finding {
    pub start: usize,
    pub end: usize,
    pub code: &'static str,
    pub message: String,
    pub severity: Severity,
    /// What the quick fix puts in place of `start..end`.
    pub replacement: Option<String>,
}

const SCHEDULE_FILES: &[&str] = &["routes/console.php", "app/Console/Kernel.php", "bootstrap/app.php"];
const ALL_INPUT: &[&str] = &["$request->all()", "request()->all()", "$request->input()", "Request::all()"];
const UNGUARDED_MARKERS: &[&str] = &["getFillable", "mergeFillable", "mergeGuarded", "#[Fillable", "#[Guarded", "#[Unguarded", "unguard", "Unguarded"];

/// `read` returns the text of a project-relative file, `None` when there is none.
pub fn findings(text: &str, read: &dyn Fn(&str) -> Option<String>) -> Vec<Finding> {
    let masked = php::mask_comments(text);
    let mut found = order_by(&masked);
    found.extend(mass_assignment(&masked, read));
    found.extend(unscheduled_prunable(&masked, read));
    found.sort_by_key(|f| f.start);
    found
}

/// `orderBy('created_at', 'desc')` and `orderBy('created_at')`, which `latest()` and `oldest()` spell shorter.
pub fn order_by(masked: &str) -> Vec<Finding> {
    let mut found = Vec::new();

    for (at, _) in masked.match_indices("orderBy") {
        let before = &masked[..at];
        if !(before.ends_with("->") || before.ends_with("::")) {
            continue;
        }

        let rest = &masked[at + 7..];
        let open = at + 7 + (rest.len() - rest.trim_start().len());
        if masked.as_bytes().get(open) != Some(&b'(') {
            continue;
        }

        let Some(close) = php::closing(masked, open) else { continue };
        let arguments = php::arguments(masked, open);
        let column = arguments.first().and_then(|range| php::literal(masked, range.clone()));
        if column.is_none_or(|(name, _)| name != "created_at") {
            continue;
        }

        let method = match arguments.get(1).map(|range| php::literal(masked, range.clone())) {
            None => "oldest",
            Some(Some((direction, _))) if direction.eq_ignore_ascii_case("asc") => "oldest",
            Some(Some((direction, _))) if direction.eq_ignore_ascii_case("desc") => "latest",
            _ => continue,
        };
        if arguments.len() > 2 {
            continue;
        }

        found.push(Finding {
            start: at,
            end: close + 1,
            code: "simplify-order-by",
            message: match method {
                "latest" => "`latest()` orders by created_at descending: use it instead of orderBy('created_at', 'desc').".into(),
                _ => "`oldest()` orders by created_at ascending: use it instead of orderBy('created_at').".into(),
            },
            severity: Severity::Hint,
            replacement: Some(format!("{method}()")),
        });
    }

    found
}

struct Model {
    fillable: Vec<String>,
    guarded_nothing: bool,
}

fn model(read: &dyn Fn(&str) -> Option<String>, class: &str) -> Option<Model> {
    let text = [format!("app/Models/{class}.php"), format!("app/{class}.php")].iter().find_map(|path| read(path))?;
    let masked = php::mask_comments(&text);
    if !masked.contains(&format!("class {class}")) || UNGUARDED_MARKERS.iter().any(|marker| masked.contains(marker)) {
        return None;
    }

    let property = |name: &str| {
        masked.match_indices(name).find_map(|(at, _)| {
            let rest = masked[at + name.len()..].trim_start().strip_prefix('=')?;
            let start = masked.len() - rest.len();
            php::array_open(&masked, start..masked.len())
        })
    };

    Some(Model {
        fillable: property("$fillable").map(|open| php::strings(&masked, open, false)).unwrap_or_default(),
        guarded_nothing: property("$guarded").is_some_and(|open| php::closing(&masked, open).is_some_and(|close| masked[open + 1..close].trim().is_empty())),
    })
}

fn mass_assignment(masked: &str, read: &dyn Fn(&str) -> Option<String>) -> Vec<Finding> {
    let mut calls: Vec<(String, usize)> = Vec::new();

    for method in ["create", "make"] {
        let needle = format!("::{method}");
        for (at, _) in masked.match_indices(&needle) {
            let open = at + needle.len();
            if masked.as_bytes().get(open) != Some(&b'(') {
                continue;
            }

            let before = &masked[..at];
            let class = before.rsplit(|c: char| !(c.is_alphanumeric() || c == '_' || c == '\\')).next().unwrap_or_default();
            let class_start = before.len() - class.len();
            if class.is_empty() || masked[..class_start].ends_with('$') {
                continue;
            }

            calls.push((class.rsplit('\\').next().unwrap_or_default().to_owned(), open));
        }
    }

    for (at, _) in masked.match_indices("->fill") {
        let open = at + 6;
        if masked.as_bytes().get(open) != Some(&b'(') {
            continue;
        }

        let receiver = masked[..at].trim_end();
        let Some(receiver) = receiver.strip_suffix(')') else { continue };
        let receiver = receiver.strip_suffix("()").unwrap_or(receiver);
        let Some((_, class)) = receiver.rsplit_once("(new ") else { continue };
        if class.is_empty() || !class.bytes().all(|b| php::is_identifier(b) || b == b'\\') {
            continue;
        }

        calls.push((class.rsplit('\\').next().unwrap_or_default().to_owned(), open));
    }

    let mut found = Vec::new();
    for (class, open) in calls {
        if matches!(class.as_str(), "static" | "self" | "parent") {
            continue;
        }

        let Some(first) = php::arguments(masked, open).into_iter().next() else { continue };
        let Some(model) = model(read, &class) else { continue };

        let argument = php::trimmed(masked, first.clone());
        if ALL_INPUT.contains(&&masked[argument.clone()]) {
            if model.guarded_nothing && model.fillable.is_empty() {
                found.push(Finding {
                    start: argument.start,
                    end: argument.end,
                    code: "guarded-attribute-assignment",
                    message: format!("{class} has `$guarded = []`, so every field of the request is mass assigned. Pass validated() data or list the attributes in `$fillable`."),
                    severity: Severity::Warning,
                    replacement: None,
                });
            }
            continue;
        }

        let Some(array) = php::array_open(masked, first) else { continue };
        if model.fillable.is_empty() {
            continue;
        }

        for item in php::items(masked, array) {
            let Some((key, offset)) = item.key.and_then(|range| php::literal(masked, range)) else { continue };
            if model.fillable.iter().any(|name| name == key) {
                continue;
            }

            found.push(Finding {
                start: offset,
                end: offset + key.len(),
                code: "guarded-attribute-assignment",
                message: format!("\"{key}\" is not in {class}::$fillable: mass assignment silently discards it."),
                severity: Severity::Warning,
                replacement: None,
            });
        }
    }

    found
}

fn unscheduled_prunable(masked: &str, read: &dyn Fn(&str) -> Option<String>) -> Vec<Finding> {
    let traits = prunable_traits(masked);
    if traits.is_empty() {
        return Vec::new();
    }

    let schedules: Vec<String> = SCHEDULE_FILES.iter().filter_map(|path| read(path)).collect();
    if schedules.is_empty() || schedules.iter().any(|text| text.contains("model:prune") || text.contains("PruneCommand")) {
        return Vec::new();
    }

    traits
        .into_iter()
        .map(|range| Finding {
            start: range.start,
            end: range.end,
            code: "unscheduled-prunable",
            message: "Prunable models are only deleted when the `model:prune` command runs: schedule it, e.g. `Schedule::command('model:prune')->daily()`.".into(),
            severity: Severity::Warning,
            replacement: None,
        })
        .collect()
}

/// The `Prunable` or `MassPrunable` names in a `use` statement inside the first class.
fn prunable_traits(masked: &str) -> Vec<Range<usize>> {
    let Some(class) = masked.match_indices("class ").find(|(at, _)| *at == 0 || masked.as_bytes()[at - 1].is_ascii_whitespace()).map(|(at, _)| at) else {
        return Vec::new();
    };

    let mut found = Vec::new();
    for (at, _) in masked[class..].match_indices("use ") {
        let at = class + at;
        let previous = masked[..at].trim_end().chars().next_back();
        if !matches!(previous, Some('{' | ';' | '}')) {
            continue;
        }

        let end = masked[at..].find([';', '{']).map_or(masked.len(), |n| at + n);
        let mut cursor = at + 4;
        for name in masked[cursor..end].split(',') {
            let start = cursor + (name.len() - name.trim_start().len());
            cursor += name.len() + 1;
            let name = name.trim();
            if matches!(name.rsplit('\\').next(), Some("Prunable" | "MassPrunable")) {
                found.push(start..start + name.len());
            }
        }
    }

    found
}
