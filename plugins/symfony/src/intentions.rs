//! Code actions that edit the open file: route attribute, route and command
//! parameters, form type class constants and Twig block overrides.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::checks;
use crate::php_text::{self, Target, Visibility};
use crate::refs;
use crate::twig;

pub struct Action {
    pub title: String,
    pub kind: &'static str,
    pub edits: Vec<(Range<usize>, String)>,
}

pub(crate) type Edit = (Range<usize>, String);

const ROUTE_ATTRIBUTE: &str = "Symfony\\Component\\Routing\\Attribute\\Route";
const ROUTE_ANNOTATION: &str = "Symfony\\Component\\Routing\\Annotation\\Route";
const ROUTE_TYPES: &[(&str, &str, &str)] = &[("Request", "Symfony\\Component\\HttpFoundation\\Request", "$request"), ("UserInterface", "Symfony\\Component\\Security\\Core\\User\\UserInterface", "$user")];
const COMMAND_TYPES: &[(&str, &str, &str)] = &[
    ("InputInterface", "Symfony\\Component\\Console\\Input\\InputInterface", "$input"),
    ("OutputInterface", "Symfony\\Component\\Console\\Output\\OutputInterface", "$output"),
    ("Cursor", "Symfony\\Component\\Console\\Cursor", "$cursor"),
    ("SymfonyStyle", "Symfony\\Component\\Console\\Style\\SymfonyStyle", "$io"),
    ("Application", "Symfony\\Component\\Console\\Application", "$application"),
];
const MODIFIERS: &[&str] = &["public", "protected", "private", "static", "final", "abstract", "readonly"];

pub(crate) struct Class {
    pub name: String,
    pub extends: Option<String>,
    pub body: Range<usize>,
}

pub(crate) struct Method {
    pub name: String,
    pub visibility: Visibility,
    pub is_static: bool,
    pub name_range: Range<usize>,
    pub span: Range<usize>,
    pub parameters: Range<usize>,
}

pub(crate) fn eol_of(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

fn last_segment(name: &str) -> &str {
    name.rsplit('\\').next().unwrap_or(name)
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

pub(crate) fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |found| found + 1)
}

pub(crate) fn indent_at(text: &str, at: usize) -> &str {
    let line = &text[line_start(text, at)..at];
    let width = line.len() - line.trim_start_matches([' ', '\t']).len();
    &line[..width]
}

pub(crate) fn find_class(masked: &str) -> Option<Class> {
    let bytes = masked.as_bytes();

    for (at, _) in masked.match_indices("class") {
        let before = at.checked_sub(1).map(|index| bytes[index]);
        let after = bytes.get(at + 5).copied();
        if before.is_some_and(|byte| is_identifier_byte(byte) || byte == b':' || byte == b'\\') || !after.is_some_and(|byte| byte.is_ascii_whitespace()) {
            continue;
        }

        let name = php_text::read_word(masked, php_text::skip_whitespace(masked, at + 5));
        if name.is_empty() || !bytes[name.start].is_ascii_alphabetic() && bytes[name.start] != b'_' {
            continue;
        }

        let open = masked[name.end..].find('{')? + name.end;
        let close = php_text::matching_close(masked, open)?;
        let header: Vec<&str> = masked[name.end..open].split_whitespace().collect();
        let extends = header.iter().position(|word| word.eq_ignore_ascii_case("extends")).and_then(|position| header.get(position + 1)).map(|name| name.to_string());

        return Some(Class { name: masked[name].to_owned(), extends, body: open + 1..close });
    }

    None
}

/// Start of the first attribute or modifier of the member whose `function` keyword is at `keyword`.
fn member_start(text: &str, masked: &str, keyword: usize) -> (usize, Visibility, bool) {
    let bytes = masked.as_bytes();
    let mut start = keyword;
    let mut visibility = Visibility::Public;
    let mut is_static = false;

    loop {
        let mut end = start;
        while end > 0 && text.as_bytes()[end - 1].is_ascii_whitespace() {
            end -= 1;
        }

        if end == 0 {
            break;
        }

        if bytes[end - 1] == b']' {
            let mut depth = 0usize;
            let mut open = None;
            for position in (0..end).rev() {
                match bytes[position] {
                    b']' => depth += 1,
                    b'[' => {
                        depth -= 1;
                        if depth == 0 {
                            open = Some(position);
                            break;
                        }
                    }
                    _ => {}
                }
            }

            match open.filter(|open| *open > 0 && bytes[open - 1] == b'#') {
                Some(open) => start = open - 1,
                None => break,
            }
            continue;
        }

        let mut word_start = end;
        while word_start > 0 && is_identifier_byte(bytes[word_start - 1]) {
            word_start -= 1;
        }

        let word = masked[word_start..end].to_ascii_lowercase();
        if !MODIFIERS.contains(&word.as_str()) {
            break;
        }

        match word.as_str() {
            "protected" => visibility = Visibility::Protected,
            "private" => visibility = Visibility::Private,
            "static" => is_static = true,
            _ => {}
        }
        start = word_start;
    }

    (start, visibility, is_static)
}

pub(crate) fn methods_of(text: &str, masked: &str, class: &Class) -> Vec<Method> {
    let bytes = masked.as_bytes();
    let mut methods = Vec::new();
    let mut depth = 0usize;
    let mut at = class.body.start;

    while at < class.body.end {
        match bytes[at] {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'f' if depth == 0 && masked[at..].starts_with("function") => {
                let before = at.checked_sub(1).map(|index| bytes[index]);
                let after = bytes.get(at + 8).copied();
                if !before.is_some_and(is_identifier_byte) && after.is_some_and(|byte| byte.is_ascii_whitespace()) {
                    if let Some(method) = method_at(text, masked, at, class.body.end) {
                        at = method.span.end.max(at + 1);
                        methods.push(method);
                        continue;
                    }
                }
            }
            _ => {}
        }
        at += 1;
    }

    methods
}

fn method_at(text: &str, masked: &str, keyword: usize, limit: usize) -> Option<Method> {
    let mut at = php_text::skip_whitespace(masked, keyword + 8);
    if masked[at..].starts_with('&') {
        at = php_text::skip_whitespace(masked, at + 1);
    }

    let name_range = php_text::read_word(masked, at);
    let open = php_text::skip_whitespace(masked, name_range.end);
    if name_range.is_empty() || !masked[open..].starts_with('(') {
        return None;
    }

    let close = php_text::matching_close(masked, open)?;
    let rest = &masked[close + 1..limit];
    let end = match rest.find(['{', ';'])? {
        found if rest.as_bytes()[found] == b'{' => php_text::matching_close(masked, close + 1 + found)? + 1,
        found => close + 1 + found + 1,
    };
    let (start, visibility, is_static) = member_start(text, masked, keyword);

    Some(Method { name: masked[name_range.clone()].to_owned(), visibility, is_static, name_range, span: start..end, parameters: open + 1..close })
}

pub(crate) fn import_for(text: &str, fqcn: &str) -> Option<(Option<Edit>, String)> {
    let edit = php_text::insert_use(text, fqcn)?;
    let import = (!edit.text.is_empty()).then(|| (edit.offset..edit.offset, edit.text));
    Some((import, edit.short_name))
}

pub fn php_actions(text: &str, at: usize, framework_version: Option<(u32, u32)>) -> Vec<Action> {
    let mut actions = Vec::new();
    let masked = php_text::mask(text);

    if let Some(class) = find_class(&masked) {
        let attributes = php_text::attributes(text);
        let class_has = |short: &str| attributes.iter().any(|attribute| matches!(&attribute.target, Target::Class { name } if *name == class.name) && last_segment(&attribute.name) == short);

        let method = methods_of(text, &masked, &class).into_iter().find(|method| method.span.contains(&at) || method.span.end == at);
        if let Some(method) = method {
            let has_route_attribute = attributes
                .iter()
                .any(|attribute| matches!(&attribute.target, Target::Method { name_range, .. } if *name_range == method.name_range) && last_segment(&attribute.name) == "Route");

            if !has_route_attribute {
                actions.extend(route_attribute_action(text, &class, &method, framework_version));
            }

            let has_route_docblock = {
                let before = text[..method.span.start].trim_end();
                before.ends_with("*/") && before.rfind("/**").is_some_and(|start| before[start..].contains("@Route"))
            };
            let invoke = method.name.eq_ignore_ascii_case("__invoke");
            let is_route_action = has_route_attribute || has_route_docblock || invoke && class_has("Route");
            let is_command = invoke && class_has("AsCommand");

            if method.visibility == Visibility::Public && !method.is_static {
                let types = match (is_command, is_route_action) {
                    (true, _) => COMMAND_TYPES,
                    (false, true) => ROUTE_TYPES,
                    (false, false) => &[],
                };
                actions.extend(types.iter().filter_map(|row| parameter_action(text, &masked, &method, *row)));
            }
        }
    }

    actions.extend(form_actions(text, at));
    actions
}

fn kebab_or_snake(name: &str, separator: char) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();

    for (position, character) in chars.iter().enumerate() {
        if *character == '_' || *character == '-' {
            out.push(separator);
            continue;
        }

        let previous = position.checked_sub(1).map(|index| chars[index]);
        let next = chars.get(position + 1);
        let boundary = character.is_uppercase()
            && previous.is_some_and(|before| before.is_lowercase() || before.is_ascii_digit() || before.is_uppercase() && next.is_some_and(|after| after.is_lowercase()));
        if boundary {
            out.push(separator);
        }
        out.extend(character.to_lowercase());
    }

    out
}

fn route_attribute_action(text: &str, class: &Class, method: &Method, framework_version: Option<(u32, u32)>) -> Option<Action> {
    let is_controller = class.name.ends_with("Controller") || class.extends.as_deref().is_some_and(|parent| matches!(last_segment(parent), "AbstractController" | "Controller"));
    let is_magic = method.name.starts_with("__");
    if !framework_version.is_some_and(|version| version >= (6, 2)) || !is_controller || is_magic || method.visibility != Visibility::Public || method.is_static {
        return None;
    }

    let existing = php_text::imports(text).into_iter().find(|import| import.fqcn == ROUTE_ATTRIBUTE || import.fqcn == ROUTE_ANNOTATION);
    let (import, route) = match existing {
        Some(import) => (None, import.alias),
        None => import_for(text, ROUTE_ATTRIBUTE)?,
    };

    let controller = class.name.strip_suffix("Controller").unwrap_or(&class.name);
    let is_index = method.name == "index";
    let parts = |separator: char| -> Vec<String> {
        let method_part = (!is_index).then(|| kebab_or_snake(&method.name, separator));
        [Some(kebab_or_snake(controller, separator)), method_part].into_iter().flatten().filter(|part| !part.is_empty()).collect()
    };
    let path = format!("/{}", parts('-').join("/"));
    let name = format!("app_{}", parts('_').join("_"));
    let attribute = format!("#[{route}('{path}', name: '{name}')]");

    let start = method.span.start;
    let edit = match text[line_start(text, start)..start].trim().is_empty() {
        true => (line_start(text, start)..line_start(text, start), format!("{}{attribute}{}", indent_at(text, start), eol_of(text))),
        false => (start..start, format!("{attribute} ")),
    };

    Some(Action { title: "Add #[Route] attribute".into(), kind: "refactor.rewrite", edits: import.into_iter().chain([edit]).collect() })
}

/// Tokens of the parameter types and names; default values are left out.
fn declared_tokens(parameters: &str) -> impl Iterator<Item = &str> {
    let mut depth = 0usize;
    let mut in_default = false;
    let mut kept = Vec::new();
    let mut start = 0;

    for (position, byte) in parameters.bytes().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b'=' if depth == 0 => {
                kept.push(&parameters[start..position]);
                in_default = true;
            }
            b',' if depth == 0 => {
                if !in_default {
                    kept.push(&parameters[start..position]);
                }
                in_default = false;
                start = position + 1;
            }
            _ => {}
        }
    }

    if !in_default {
        kept.push(&parameters[start..]);
    }

    kept.into_iter().flat_map(tokens)
}

fn tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(|character: char| !(character.is_alphanumeric() || matches!(character, '_' | '\\' | '$')))
        .filter(|token| !token.is_empty())
}

fn parameter_action(text: &str, masked: &str, method: &Method, (short, fqcn, variable): (&str, &str, &str)) -> Option<Action> {
    let parameters = &masked[method.parameters.clone()];
    if declared_tokens(parameters).any(|token| token.trim_start_matches('\\') == short || token.trim_start_matches('\\') == fqcn || token == variable) {
        return None;
    }

    let (import, name) = import_for(text, fqcn)?;
    let declaration = format!("{name} {variable}");
    let eol = eol_of(text);
    let start = method.parameters.start;

    let edit = match parameters.trim().is_empty() {
        true => (start..method.parameters.end, declaration),
        false => {
            let content = parameters.trim_end();
            let end = start + content.len();
            let trailing_comma = content.ends_with(',');
            let body = content.trim_end_matches(',');

            let mut depth = 0usize;
            let mut last_start = 0;
            for (position, byte) in body.bytes().enumerate() {
                match byte {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => depth = depth.saturating_sub(1),
                    b',' if depth == 0 => last_start = position + 1,
                    _ => {}
                }
            }

            let last = php_text::skip_whitespace(masked, start + last_start);
            let separator = match parameters.contains('\n') {
                true => format!("{eol}{}", indent_at(text, last)),
                false => " ".to_owned(),
            };
            let insertion = match trailing_comma {
                true => format!("{separator}{declaration},"),
                false => format!(",{separator}{declaration}"),
            };
            let end = if trailing_comma { end } else { start + body.len() };
            (end..end, insertion)
        }
    };

    Some(Action { title: format!("Add parameter: {short} {variable}"), kind: "refactor.rewrite", edits: import.into_iter().chain([edit]).collect() })
}

fn form_class(class: &str) -> String {
    match class {
        "EntityType" => "Symfony\\Bridge\\Doctrine\\Form\\Type\\EntityType".to_owned(),
        _ => format!("Symfony\\Component\\Form\\Extension\\Core\\Type\\{class}"),
    }
}

fn form_actions(text: &str, at: usize) -> Vec<Action> {
    let literals = checks::form_type_literals(text);
    let mut actions = Vec::new();

    let convertible: Vec<(Range<usize>, String, Option<Edit>)> = literals
        .iter()
        .filter_map(|(range, _, class)| {
            let (import, name) = import_for(text, &form_class(class))?;
            Some((range.start - 1..range.end + 1, name, import))
        })
        .map(|(range, name, import)| (range, format!("{name}::class"), import))
        .collect();

    if let Some((range, replacement, import)) = convertible.iter().find(|(range, ..)| range.contains(&at) || range.end == at) {
        actions.push(Action {
            title: format!("Use {replacement}"),
            kind: "refactor.rewrite",
            edits: import.iter().cloned().chain([(range.clone(), replacement.clone())]).collect(),
        });
    }

    if convertible.len() >= 2 {
        let mut imports: BTreeMap<String, Edit> = BTreeMap::new();
        for (_, replacement, import) in &convertible {
            if let Some(import) = import {
                imports.entry(replacement.clone()).or_insert_with(|| import.clone());
            }
        }

        let offset = imports.values().next().map_or(0, |(range, _)| range.start);
        let import_text: String = imports
            .values()
            .enumerate()
            .map(|(position, (_, line))| match position {
                0 => line.as_str(),
                _ => line.trim_start_matches(['\r', '\n']),
            })
            .collect();
        let mut edits: Vec<Edit> = Vec::new();
        if !import_text.is_empty() {
            edits.push((offset..offset, import_text));
        }
        edits.extend(convertible.iter().map(|(range, replacement, _)| (range.clone(), replacement.clone())));

        actions.push(Action { title: "Use class constants for all form types in this file".into(), kind: "refactor.rewrite", edits });
    }

    actions
}

/// Whether `at` is on a `{% extends %}` tag that is not inside a comment or verbatim body.
pub fn caret_on_extends(text: &str, at: usize) -> bool {
    let mut end = (at + 2).min(text.len());
    while !text.is_char_boundary(end) {
        end += 1;
    }

    let Some(open) = text[..end].rfind("{%") else { return false };
    if !twig::code_regions(text).iter().any(|region| region.contains(&open)) {
        return false;
    }

    let Some(close) = text[open + 2..].find("%}").map(|found| open + 2 + found + 2) else { return false };
    let inside = text[open + 2..close - 2].trim_start_matches(['-', '~']).trim_start();
    let is_extends = inside.strip_prefix("extends").is_some_and(|rest| !rest.bytes().next().is_some_and(refs::is_identifier));

    at <= close && is_extends
}

pub fn twig_actions(text: &str, at: usize, parent_blocks: &[String]) -> Vec<Action> {
    if !caret_on_extends(text, at) {
        return Vec::new();
    }

    let defined: Vec<&str> = refs::twig_blocks(text).into_iter().map(|(name, _)| name).collect();
    let eol = eol_of(text);
    let separator = if text.is_empty() || text.ends_with('\n') { "" } else { eol };
    let mut offered: Vec<&str> = Vec::new();

    parent_blocks
        .iter()
        .filter(|block| !defined.contains(&block.as_str()))
        .filter(|block| {
            let fresh = !offered.contains(&block.as_str());
            offered.push(block);
            fresh
        })
        .map(|block| Action {
            title: format!("Override block \"{block}\""),
            kind: "refactor.rewrite",
            edits: vec![(text.len()..text.len(), format!("{separator}{eol}{{% block {block} %}}{eol}    {{{{ parent() }}}}{eol}{{% endblock %}}{eol}"))],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn caret_on_extends_survives_multibyte_text_after_the_caret() {
        let text = "{% extends '\u{e9}' %}\n";
        assert!(super::caret_on_extends(text, "{% extends '".len()));
        let text = "{% extends \u{2019}'a' %}\n";
        assert!(super::caret_on_extends(text, "{% extends ".len()));
        let text = "{% extends 'a' %}\n\u{1F600}";
        assert!(super::caret_on_extends(text, "{% extends".len()));
        let text = "{% extends 'a' %}\u{e9}";
        assert!(super::caret_on_extends(text, "{% extends 'a' %}".len() - 1));
        let text = "{% e\u{2019}";
        assert!(!super::caret_on_extends(text, 3));
        assert!(!super::caret_on_extends("{%}", 1));
        let text = "{%\u{2019}";
        assert!(!super::caret_on_extends(text, 1));
    }

    use super::*;

    const V7: Option<(u32, u32)> = Some((7, 0));

    fn apply(text: &str, action: &Action) -> String {
        let mut edits: Vec<&(Range<usize>, String)> = action.edits.iter().collect();
        edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        let mut out = text.to_owned();
        for (range, replacement) in edits {
            out.replace_range(range.clone(), replacement);
        }
        out
    }

    fn titles(actions: &[Action]) -> Vec<&str> {
        actions.iter().map(|action| action.title.as_str()).collect()
    }

    fn caret(text: &str, marker: &str) -> usize {
        text.find(marker).unwrap()
    }

    fn find<'a>(actions: &'a [Action], title: &str) -> &'a Action {
        actions.iter().find(|action| action.title == title).unwrap_or_else(|| panic!("no action {title}: {:?}", titles(actions)))
    }

    const CONTROLLER: &str = "<?php\nnamespace App\\Controller;\n\nclass BlogController extends AbstractController\n{\n    /** Shows. */\n    public function showPost(int $id)\n    {\n        return 1;\n    }\n}\n";

    #[test]
    fn route_attribute_is_added_above_a_public_method() {
        let actions = php_actions(CONTROLLER, caret(CONTROLLER, "return 1"), V7);
        let action = find(&actions, "Add #[Route] attribute");
        assert_eq!(action.kind, "refactor.rewrite");
        let expected = "<?php\nnamespace App\\Controller;\n\nuse Symfony\\Component\\Routing\\Attribute\\Route;\n\nclass BlogController extends AbstractController\n{\n    /** Shows. */\n    #[Route('/blog/show-post', name: 'app_blog_show_post')]\n    public function showPost(int $id)\n";
        assert!(apply(CONTROLLER, action).starts_with(expected), "{}", apply(CONTROLLER, action));
    }

    #[test]
    fn route_attribute_goes_above_the_first_attribute_and_modifier() {
        let text = "<?php\nuse Symfony\\Component\\Routing\\Attribute\\Route;\nclass PostController\n{\n    #[Other]\n    final public function list() {}\n}\n";
        let actions = php_actions(text, caret(text, "list"), V7);
        let out = apply(text, find(&actions, "Add #[Route] attribute"));
        assert!(out.contains("{\n    #[Route('/post/list', name: 'app_post_list')]\n    #[Other]\n    final public function list()"), "{out}");
    }

    #[test]
    fn index_method_gets_the_controller_path_and_name() {
        let text = "<?php\nclass BlogPostController extends AbstractController\n{\n    public function index() {}\n}\n";
        let actions = php_actions(text, caret(text, "index"), V7);
        let out = apply(text, find(&actions, "Add #[Route] attribute"));
        assert!(out.contains("#[Route('/blog-post', name: 'app_blog_post')]"), "{out}");
    }

    #[test]
    fn route_attribute_is_quiet_for_private_static_magic_or_already_routed_methods() {
        let text = "<?php\nclass BlogController\n{\n    private function a() {}\n    public static function b() {}\n    public function __construct() {}\n    #[Route('/x')]\n    public function c() {}\n    #[Symfony\\Component\\Routing\\Annotation\\Route('/y')]\n    public function d() {}\n}\n";
        for name in ["function a", "function b", "function __construct", "function c", "function d"] {
            let actions = php_actions(text, caret(text, name), V7);
            assert!(!titles(&actions).contains(&"Add #[Route] attribute"), "{name}");
        }
    }

    #[test]
    fn route_attribute_is_quiet_outside_controllers() {
        let text = "<?php\nclass Mailer\n{\n    public function send() {}\n}\n";
        assert!(php_actions(text, caret(text, "send"), V7).is_empty());
    }

    #[test]
    fn route_attribute_is_quiet_below_symfony_6_2() {
        let at = caret(CONTROLLER, "return 1");
        for version in [None, Some((6, 1)), Some((5, 4))] {
            assert!(php_actions(CONTROLLER, at, version).is_empty());
        }
        assert!(!php_actions(CONTROLLER, at, Some((6, 2))).is_empty());
    }

    #[test]
    fn route_attribute_reuses_an_existing_route_import() {
        let text = "<?php\nuse Symfony\\Component\\Routing\\Annotation\\Route as R;\nclass BlogController\n{\n    public function show() {}\n}\n";
        let actions = php_actions(text, caret(text, "show"), V7);
        let out = apply(text, find(&actions, "Add #[Route] attribute"));
        assert_eq!(out.matches("use ").count(), 1, "{out}");
        assert!(out.contains("#[R('/blog/show', name: 'app_blog_show')]"), "{out}");
    }

    #[test]
    fn route_attribute_is_not_offered_on_an_unrelated_route_import() {
        let text = "<?php\nuse Other\\Route;\nclass BlogController\n{\n    public function show() {}\n}\n";
        assert!(!titles(&php_actions(text, caret(text, "show"), V7)).contains(&"Add #[Route] attribute"));
    }

    const ACTION: &str = "<?php\nnamespace App\\Controller;\n\nclass BlogController\n{\n    #[Route('/a')]\n    public function show(int $id): Response\n    {\n    }\n}\n";

    #[test]
    fn adds_request_parameter_to_a_single_line_signature() {
        let actions = php_actions(ACTION, caret(ACTION, "show"), Some((5, 0)));
        assert_eq!(titles(&actions), ["Add parameter: Request $request", "Add parameter: UserInterface $user"]);
        let out = apply(ACTION, &actions[0]);
        assert!(out.contains("use Symfony\\Component\\HttpFoundation\\Request;"), "{out}");
        assert!(out.contains("show(int $id, Request $request): Response"), "{out}");
    }

    #[test]
    fn adds_parameter_to_a_multi_line_signature() {
        let text = "<?php\nclass A\n{\n    #[Route('/a')]\n    public function show(\n        int $id,\n        string $slug = 'x'\n    ) {\n    }\n}\n";
        let actions = php_actions(text, caret(text, "show"), V7);
        let out = apply(text, find(&actions, "Add parameter: Request $request"));
        assert!(out.contains("        string $slug = 'x',\n        Request $request\n    ) {"), "{out}");

        let trailing = "<?php\nclass A\n{\n    #[Route('/a')]\n    public function show(\n        int $id,\n    ) {\n    }\n}\n";
        let actions = php_actions(trailing, caret(trailing, "show"), V7);
        let out = apply(trailing, find(&actions, "Add parameter: UserInterface $user"));
        assert!(out.contains("        int $id,\n        UserInterface $user,\n    ) {"), "{out}");
    }

    #[test]
    fn adds_parameter_to_an_empty_list() {
        let text = "<?php\nclass A\n{\n    #[Route('/a')]\n    public function show() {}\n}\n";
        let actions = php_actions(text, caret(text, "show"), V7);
        let out = apply(text, find(&actions, "Add parameter: Request $request"));
        assert!(out.contains("public function show(Request $request) {}"), "{out}");
    }

    #[test]
    fn skips_a_type_already_present_by_short_or_full_name() {
        let text = "<?php\nclass A\n{\n    #[Route('/a')]\n    public function show(\\Symfony\\Component\\HttpFoundation\\Request $r, ?UserInterface $u) {}\n}\n";
        assert!(php_actions(text, caret(text, "show"), V7).is_empty());

        let by_variable = "<?php\nclass A\n{\n    #[Route('/a')]\n    public function show(string $request) {}\n}\n";
        let actions = php_actions(by_variable, caret(by_variable, "show"), V7);
        assert_eq!(titles(&actions), ["Add parameter: UserInterface $user"]);
    }

    #[test]
    fn route_parameter_is_quiet_outside_route_actions() {
        let text = "<?php\nclass A\n{\n    public function show() {}\n\n    #[Route('/a')]\n    private function hidden() {}\n}\n";
        assert!(php_actions(text, caret(text, "show"), V7).is_empty());
        assert!(php_actions(text, caret(text, "hidden"), V7).is_empty());
    }

    #[test]
    fn invoke_of_a_routed_class_and_docblock_routes_are_actions() {
        let invokable = "<?php\n#[Route('/a')]\nclass A\n{\n    public function __invoke() {}\n}\n";
        assert_eq!(php_actions(invokable, caret(invokable, "__invoke"), V7).len(), 2);

        let docblock = "<?php\nclass A\n{\n    /**\n     * @Route(\"/a\")\n     */\n    public function show() {}\n}\n";
        assert_eq!(php_actions(docblock, caret(docblock, "show"), V7).len(), 2);
    }

    #[test]
    fn command_invoke_offers_the_five_console_types() {
        let text = "<?php\nnamespace App\\Command;\n\n#[AsCommand(name: 'app:x')]\nclass XCommand\n{\n    public function __invoke(): int\n    {\n        return 0;\n    }\n}\n";
        let actions = php_actions(text, caret(text, "return 0"), V7);
        assert_eq!(
            titles(&actions),
            [
                "Add parameter: InputInterface $input",
                "Add parameter: OutputInterface $output",
                "Add parameter: Cursor $cursor",
                "Add parameter: SymfonyStyle $io",
                "Add parameter: Application $application",
            ]
        );
        let out = apply(text, find(&actions, "Add parameter: Cursor $cursor"));
        assert!(out.contains("use Symfony\\Component\\Console\\Cursor;"), "{out}");
        assert!(out.contains("__invoke(Cursor $cursor): int"), "{out}");
    }

    #[test]
    fn parameter_action_is_not_offered_on_an_import_clash() {
        let text = "<?php\nuse Foo\\Request;\nclass A\n{\n    #[Route('/a')]\n    public function show() {}\n}\n";
        assert_eq!(titles(&php_actions(text, caret(text, "show"), V7)), ["Add parameter: UserInterface $user"]);

        let no_open_tag = "class A\n{\n    #[Route('/a')]\n    public function show() {}\n}\n";
        assert!(php_actions(no_open_tag, caret(no_open_tag, "show"), V7).is_empty());
    }

    const FORM: &str = "<?php\nnamespace App\\Form;\n\nclass PostType extends AbstractType\n{\n    public function buildForm(FormBuilderInterface $builder, array $options)\n    {\n        $builder->add('title', 'text');\n    }\n}\n";

    #[test]
    fn form_string_alias_becomes_a_class_constant_with_import() {
        let actions = php_actions(FORM, caret(FORM, "'text'") + 2, V7);
        assert_eq!(titles(&actions), ["Use TextType::class"]);
        assert_eq!(actions[0].kind, "refactor.rewrite");
        let out = apply(FORM, &actions[0]);
        assert!(out.contains("$builder->add('title', TextType::class);"), "{out}");
        assert!(out.contains("namespace App\\Form;\n\nuse Symfony\\Component\\Form\\Extension\\Core\\Type\\TextType;\n"), "{out}");

        let entity = FORM.replace("'text'", "\"entity\"");
        let actions = php_actions(&entity, caret(&entity, "\"entity\"") + 1, V7);
        assert!(apply(&entity, &actions[0]).contains("use Symfony\\Bridge\\Doctrine\\Form\\Type\\EntityType;"));
    }

    #[test]
    fn form_string_whole_file_converts_every_alias_once_per_import() {
        let text = "<?php\nnamespace App\\Form;\n\nclass PostType extends AbstractType\n{\n    public function buildForm(FormBuilderInterface $builder, array $options)\n    {\n        $builder->add('a', 'text')->add('b', 'text')->add('c', 'checkbox');\n    }\n}\n";
        let actions = php_actions(text, caret(text, "'a'"), V7);
        let whole = find(&actions, "Use class constants for all form types in this file");
        let out = apply(text, whole);
        assert!(out.contains("->add('a', TextType::class)->add('b', TextType::class)->add('c', CheckboxType::class)"), "{out}");
        assert_eq!(out.matches("Type\\TextType;").count(), 1, "{out}");
        assert!(out.contains("\\CheckboxType;"), "{out}");

        assert!(!titles(&php_actions(FORM, caret(FORM, "'text'"), V7)).contains(&"Use class constants for all form types in this file"));
    }

    #[test]
    fn form_string_is_quiet_for_unknown_aliases_and_dynamic_strings() {
        for replacement in ["'custom'", "$type", "'te' . 'xt'", "TextType::class"] {
            let text = FORM.replace("'text'", replacement);
            let at = caret(&text, "->add(") + 12;
            assert!(php_actions(&text, at, V7).is_empty(), "{replacement}");
        }

        let first_argument = FORM.replace("'title'", "'text'");
        assert!(php_actions(&first_argument, caret(&first_argument, "'text'") + 1, V7).is_empty());
    }

    const CHILD: &str = "{% extends 'base.html.twig' %}\n{% block body %}x{% endblock %}";

    fn parents() -> Vec<String> {
        ["title", "body", "javascripts"].map(String::from).to_vec()
    }

    #[test]
    fn block_override_is_offered_per_missing_parent_block_on_the_extends_tag() {
        let actions = twig_actions(CHILD, 5, &parents());
        assert_eq!(titles(&actions), ["Override block \"title\"", "Override block \"javascripts\""]);
        assert_eq!(actions[0].kind, "refactor.rewrite");
        assert_eq!(apply(CHILD, &actions[0]), format!("{CHILD}\n\n{{% block title %}}\n    {{{{ parent() }}}}\n{{% endblock %}}\n"));

        let with_newline = format!("{CHILD}\n");
        assert_eq!(apply(&with_newline, &twig_actions(&with_newline, 0, &parents())[0]), format!("{CHILD}\n\n{{% block title %}}\n    {{{{ parent() }}}}\n{{% endblock %}}\n"));
    }

    #[test]
    fn block_override_is_quiet_for_blocks_already_defined() {
        let all = "{% extends 'b' %}{% block title %}{% endblock %}{% block body %}{% endblock %}{% block javascripts %}{% endblock %}";
        assert!(twig_actions(all, 3, &parents()).is_empty());
    }

    #[test]
    fn block_override_is_quiet_away_from_the_extends_tag() {
        assert!(twig_actions(CHILD, CHILD.len() - 3, &parents()).is_empty());
        assert!(twig_actions(CHILD, caret(CHILD, "block body"), &parents()).is_empty());
        let no_extends = "{% block body %}x{% endblock %}";
        assert!(twig_actions(no_extends, 3, &parents()).is_empty());
    }

    #[test]
    fn intentions_apply_twice_without_duplicating_imports() {
        let first = apply(ACTION, &php_actions(ACTION, caret(ACTION, "show"), V7)[0]);
        let again = php_actions(&first, caret(&first, "show"), V7);
        let second = apply(&first, find(&again, "Add parameter: UserInterface $user"));
        assert_eq!(second.matches("use Symfony\\Component\\HttpFoundation\\Request;").count(), 1);
        assert!(second.contains("Request $request, UserInterface $user"), "{second}");
        assert!(!titles(&again).contains(&"Add parameter: Request $request"));
    }

    #[test]
    fn intentions_survive_multibyte_text_and_crlf() {
        let text = "<?php\r\nnamespace App;\r\n\r\n// été 日本\r\nclass BlogController extends AbstractController\r\n{\r\n    public function showé(\r\n        int $id\r\n    ) {\r\n    }\r\n}\r\n";
        let actions = php_actions(text, caret(text, "showé"), V7);
        let out = apply(text, find(&actions, "Add #[Route] attribute"));
        assert!(out.contains("namespace App;\r\n\r\nuse Symfony\\Component\\Routing\\Attribute\\Route;\r\n"), "{out:?}");
        assert!(out.contains("    #[Route('/blog/showé', name: 'app_blog_showé')]\r\n    public function showé("), "{out:?}");
        assert!(!out.replace("\r\n", "").contains('\n'));

        let routed = out;
        let actions = php_actions(&routed, caret(&routed, "showé("), V7);
        let out = apply(&routed, find(&actions, "Add parameter: Request $request"));
        assert!(out.contains("        int $id,\r\n        Request $request\r\n    ) {"), "{out:?}");

        let twig = "{% extends 'a' %}\r\né";
        let out = apply(twig, &twig_actions(twig, 3, &["x".to_owned()])[0]);
        assert!(out.ends_with("\r\né\r\n\r\n{% block x %}\r\n    {{ parent() }}\r\n{% endblock %}\r\n"), "{out:?}");
    }

    #[test]
    fn caret_on_extends_is_a_cheap_predicate_for_the_extends_tag_only() {
        assert!(caret_on_extends(CHILD, 5));
        assert!(!caret_on_extends(CHILD, CHILD.len() - 3));
        assert!(!caret_on_extends(CHILD, caret(CHILD, "block body")));
        assert!(!caret_on_extends("plain text", 3));
    }

    #[test]
    fn block_override_is_quiet_inside_comments_and_verbatim() {
        let commented = "{# {% extends 'b' %} #}\n";
        assert!(twig_actions(commented, 8, &parents()).is_empty());
        let verbatim = "{% verbatim %}{% extends 'b' %}{% endverbatim %}";
        assert!(twig_actions(verbatim, 18, &parents()).is_empty());
    }

    #[test]
    fn parameter_defaults_do_not_count_as_declared_types() {
        let text = "<?php\nclass A\n{\n    #[Route('/a')]\n    public function show(string $class = Request::class, array $u = [UserInterface::class]) {}\n}\n";
        let actions = php_actions(text, caret(text, "show"), V7);
        assert_eq!(titles(&actions), ["Add parameter: Request $request", "Add parameter: UserInterface $user"]);
    }
}
