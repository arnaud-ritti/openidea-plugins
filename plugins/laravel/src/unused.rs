//! Views no code refers to, and the PHPDoc blocks IDE helper writes into models.

use std::collections::BTreeSet;
use std::ops::Range;

use crate::refs::{self, Kind};

const FRAMEWORK_PREFIXES: &[&str] = &["components.", "errors.", "vendor.", "livewire.", "pagination.", "emails.", "mail."];
const DYNAMIC_CALLS: &[&str] = &["view($", "View::make($", "@include($", "@extends($", "->markdown($", "->view($", "Route::view($", "view(\"{$", "@includeIf($"];

pub struct Unused {
    pub name: String,
    pub maybe: bool,
}

/// Views of `views` that none of `sources` (`(language, text)`) names. `maybe`
/// is set when some source builds view names at run time.
pub fn unused_views(views: &BTreeSet<&str>, sources: &[(&str, String)]) -> Vec<Unused> {
    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut prefixes: Vec<String> = Vec::new();
    let mut dynamic = false;

    for (language, text) in sources {
        dynamic |= DYNAMIC_CALLS.iter().any(|call| text.contains(call));

        for reference in refs::references(language, text).into_iter().filter(|r| r.kind == Kind::View) {
            let name = &text[reference.start..reference.end];
            if name.contains(['$', '{', '*']) {
                prefixes.push(name.split(['$', '{', '*']).next().unwrap_or_default().to_owned());
                continue;
            }

            if !reference.closed || name.ends_with('.') {
                prefixes.push(name.to_owned());
                continue;
            }

            used.insert(name.to_owned());
        }
    }

    views
        .iter()
        .filter(|name| !used.contains(**name))
        .filter(|name| !name.contains("::"))
        .filter(|name| !FRAMEWORK_PREFIXES.iter().any(|prefix| name.starts_with(prefix)))
        .filter(|name| !prefixes.iter().any(|prefix| !prefix.is_empty() && name.starts_with(prefix.as_str())))
        .map(|name| Unused { name: (*name).to_owned(), maybe: dynamic })
        .collect()
}

fn is_helper_tag(line: &str) -> bool {
    let tag = line.trim().trim_start_matches('*').trim();
    ["@property ", "@property-read ", "@property-write ", "@method ", "@mixin "].iter().any(|prefix| tag.starts_with(prefix))
}

/// The edit that removes the `@property`, `@method` and `@mixin` lines of the
/// docblock above the model class around `at`, and the block itself when
/// nothing else is left in it.
pub fn remove_model_phpdocs(text: &str, at: usize) -> Option<(Range<usize>, String)> {
    let class_at = text.match_indices("class ").find_map(|(i, _)| {
        let line_start = text[..i].rfind('\n').map_or(0, |l| l + 1);
        let before = text[line_start..i].trim();
        let line_end = text[i..].find('\n').map_or(text.len(), |l| i + l);
        let declaration = &text[i..line_end];
        let extends_model = declaration.contains(" extends Model") || declaration.contains(" extends Authenticatable") || declaration.contains(" extends Pivot");
        (before.split_whitespace().all(|w| matches!(w, "final" | "abstract")) && extends_model).then_some(line_start)
    })?;

    let block_end = text[..class_at].trim_end();
    if !block_end.ends_with("*/") {
        return None;
    }

    let block_start = block_end.rfind("/**")?;
    let line_start = text[..block_start].rfind('\n').map_or(0, |l| l + 1);
    if !text[line_start..block_start].trim().is_empty() {
        return None;
    }

    let class_line_end = text[class_at..].find('\n').map_or(text.len(), |l| class_at + l);
    if at < line_start || at > class_line_end {
        return None;
    }

    let block = &text[line_start..block_end.len()];
    if !block.lines().any(is_helper_tag) {
        return None;
    }

    let kept: Vec<&str> = block.lines().filter(|line| !is_helper_tag(line)).collect();
    let empty = kept.iter().all(|line| {
        let content = line.trim().trim_start_matches("/**").trim_end_matches("*/").trim().trim_start_matches('*').trim();
        content.is_empty() || content.starts_with("App\\") || content.starts_with("Class ")
    });

    let replacement = if empty { String::new() } else { format!("{}\n", kept.join("\n")) };
    let end = if empty { class_at } else { block_end.len() + 1 };
    Some((line_start..end, replacement))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn views<'a>(names: &[&'a str]) -> BTreeSet<&'a str> {
        names.iter().copied().collect()
    }

    #[test]
    fn views_nobody_names_are_reported() {
        let sources = [
            ("php", "return view('posts.index');".to_owned()),
            ("blade", "@extends('layouts.app')\n@include('partials.nav')".to_owned()),
        ];
        let found = unused_views(&views(&["posts.index", "layouts.app", "partials.nav", "old.page", "errors.404", "components.card"]), &sources);
        let names: Vec<&str> = found.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["old.page"]);
        assert!(!found[0].maybe);
    }

    #[test]
    fn dynamic_names_make_the_result_a_maybe_and_cover_their_prefix() {
        let sources = [("php", "return view('reports.' . $type);\nreturn view($name);".to_owned())];
        let found = unused_views(&views(&["reports.daily", "other"]), &sources);
        let names: Vec<(&str, bool)> = found.iter().map(|u| (u.name.as_str(), u.maybe)).collect();
        assert_eq!(names, [("other", true)]);
    }

    #[test]
    fn mail_markdown_and_route_view_count_as_used() {
        let sources = [("php", "Route::view('/about', 'about');\n$m->markdown('mail.order');\nnew Content(view: 'invoice');".to_owned())];
        let found = unused_views(&views(&["about", "invoice", "orphan"]), &sources);
        assert_eq!(found.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(), ["orphan"]);
    }

    const MODEL: &str = "<?php\n\nnamespace App\\Models;\n\n/**\n * App\\Models\\Post\n *\n * @property int $id\n * @property string $title\n * @method static \\Illuminate\\Database\\Eloquent\\Builder|Post query()\n * @mixin \\Eloquent\n */\nclass Post extends Model\n{\n}\n";

    #[test]
    fn helper_only_docblock_is_removed_with_the_block() {
        let at = MODEL.find("class Post").unwrap();
        let (range, replacement) = remove_model_phpdocs(MODEL, at).unwrap();
        let mut out = MODEL.to_owned();
        out.replace_range(range, &replacement);
        assert_eq!(out, "<?php\n\nnamespace App\\Models;\n\nclass Post extends Model\n{\n}\n");
    }

    #[test]
    fn handwritten_lines_survive() {
        let text = "<?php\n/**\n * A blog post.\n *\n * @property int $id\n */\nclass Post extends Model {}\n";
        let (range, replacement) = remove_model_phpdocs(text, text.find("class").unwrap()).unwrap();
        let mut out = text.to_owned();
        out.replace_range(range, &replacement);
        assert_eq!(out, "<?php\n/**\n * A blog post.\n *\n */\nclass Post extends Model {}\n");
    }

    #[test]
    fn plain_classes_and_far_carets_are_left_alone() {
        let text = "<?php\n/**\n * @property int $id\n */\nclass Post extends Base {}\n";
        assert!(remove_model_phpdocs(text, text.find("class").unwrap()).is_none());
        assert!(remove_model_phpdocs(MODEL, MODEL.len() - 2).is_none());
    }
}
