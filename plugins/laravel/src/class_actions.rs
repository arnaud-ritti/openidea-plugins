//! Class rewrites: `scopeX` to `#[Scope]`, a method argument to a promoted
//! constructor property, and a facade call to an injected dependency.

use std::ops::Range;

use crate::actions::Rewrite;
use crate::php::{self, is_identifier};

/// Facade, the class the Laravel documentation lists as its underlying class, and the property name.
const FACADE_CLASSES: &[(&str, &str)] = &[
    ("Artisan", "Illuminate\\Contracts\\Console\\Kernel"),
    ("Auth", "Illuminate\\Auth\\AuthManager"),
    ("Cache", "Illuminate\\Cache\\CacheManager"),
    ("Config", "Illuminate\\Config\\Repository"),
    ("Cookie", "Illuminate\\Cookie\\CookieJar"),
    ("Crypt", "Illuminate\\Encryption\\Encrypter"),
    ("DB", "Illuminate\\Database\\DatabaseManager"),
    ("Event", "Illuminate\\Events\\Dispatcher"),
    ("File", "Illuminate\\Filesystem\\Filesystem"),
    ("Gate", "Illuminate\\Contracts\\Auth\\Access\\Gate"),
    ("Hash", "Illuminate\\Hashing\\HashManager"),
    ("Http", "Illuminate\\Http\\Client\\Factory"),
    ("Lang", "Illuminate\\Translation\\Translator"),
    ("Log", "Illuminate\\Log\\LogManager"),
    ("Mail", "Illuminate\\Mail\\Mailer"),
    ("Queue", "Illuminate\\Queue\\QueueManager"),
    ("Redirect", "Illuminate\\Routing\\Redirector"),
    ("Session", "Illuminate\\Session\\SessionManager"),
    ("Storage", "Illuminate\\Filesystem\\FilesystemManager"),
    ("URL", "Illuminate\\Routing\\UrlGenerator"),
    ("Validator", "Illuminate\\Validation\\Factory"),
    ("View", "Illuminate\\View\\Factory"),
];

const SAFE_PARENTS: &[&str] = &["Controller"];

struct Class {
    open: usize,
    close: usize,
    parent: Option<String>,
}

struct Method {
    line_start: usize,
    name: String,
    is_static: bool,
    params: Range<usize>,
    body: Range<usize>,
}

fn class_of(masked: &str) -> Option<Class> {
    let at = masked.match_indices("class ").find_map(|(at, _)| {
        let line_start = masked[..at].rfind('\n').map_or(0, |i| i + 1);
        masked[line_start..at].split_whitespace().all(|word| matches!(word, "final" | "abstract" | "readonly")).then_some(at)
    })?;
    let open = masked[at..].find('{').map(|i| at + i)?;
    let close = php::closing(masked, open)?;
    let header: Vec<&str> = masked[at..open].split_whitespace().collect();
    let parent = header.iter().position(|word| *word == "extends").and_then(|i| header.get(i + 1)).map(|p| p.trim_start_matches('\\').rsplit('\\').next().unwrap_or_default().to_owned());

    Some(Class { open, close, parent })
}

fn methods(masked: &str, class: &Class) -> Vec<Method> {
    let mut found = Vec::new();
    let mut from = class.open;

    while let Some(at) = masked[from..class.close].find("function ").map(|i| from + i) {
        from = at + "function ".len();
        let line_start = masked[..at].rfind('\n').map_or(0, |i| i + 1);
        let modifiers = &masked[line_start..at];
        if modifiers.contains("=>") || modifiers.contains('(') || modifiers.contains('=') {
            continue;
        }

        let name: String = masked[from..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        let open = from + name.len();
        if name.is_empty() || !masked[open..].starts_with('(') {
            continue;
        }

        let Some(close) = php::closing(masked, open) else { continue };
        let Some(brace) = masked[close..class.close].find(['{', ';']).map(|i| close + i).filter(|i| masked.as_bytes()[*i] == b'{') else { continue };
        let Some(end) = php::closing(masked, brace) else { continue };

        found.push(Method { line_start, name, is_static: modifiers.split_whitespace().any(|w| w == "static"), params: open + 1..close, body: brace..end + 1 });
        from = end;
    }

    found
}

fn indentation(text: &str, line_start: usize) -> &str {
    let line = &text[line_start..];
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |i| at + i + 1)
}

fn camel(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| first.to_lowercase().chain(chars).collect())
}

fn whole_word_positions(haystack: &str, word: &str) -> Vec<usize> {
    let bytes = haystack.as_bytes();
    haystack
        .match_indices(word)
        .map(|(at, _)| at)
        .filter(|at| !bytes.get(at + word.len()).is_some_and(|b| is_identifier(*b)))
        .collect()
}

/// Edits adding `private <ty> $<name>` to the constructor, creating it when the class has none.
fn constructor_edits(text: &str, masked: &str, class: &Class, found: &[Method], ty: &str, name: &str) -> Option<Vec<(Range<usize>, String)>> {
    if class.parent.as_deref().is_some_and(|parent| !SAFE_PARENTS.contains(&parent)) {
        return None;
    }

    let parameter = format!("private {ty} ${name}");
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };

    if let Some(constructor) = found.iter().find(|m| m.name == "__construct") {
        let existing = &masked[constructor.params.clone()];
        if !whole_word_positions(existing, &format!("${name}")).is_empty() {
            return None;
        }

        let arguments = php::split(masked, constructor.params.clone());
        let Some(last) = arguments.last() else { return Some(vec![(constructor.params.clone(), parameter)]) };
        let end = php::trimmed(masked, last.clone()).end;
        let multiline = existing.contains('\n');
        let has_trailing_comma = masked[end..constructor.params.end].trim_start().starts_with(',');
        let indent = indentation(text, text[..last.start].trim_end().rfind('\n').map_or(0, |i| i + 1)).to_owned();
        let inserted = match (multiline, has_trailing_comma) {
            (true, true) => return Some(vec![(end + 1..end + 1, format!("{eol}{indent}{parameter},"))]),
            (true, false) => format!(",{eol}{indent}{parameter}"),
            (false, _) => format!(", {parameter}"),
        };
        return Some(vec![(end..end, inserted)]);
    }

    let anchor = found.first()?;
    let mut insert_at = anchor.line_start;
    loop {
        let before = text[..insert_at].trim_end_matches(['\n', '\r']);
        let previous_start = before.rfind('\n').map_or(0, |i| i + 1);
        let previous = before[previous_start..].trim();
        if previous.ends_with("*/") || previous.starts_with('*') || previous.starts_with("/**") || previous.starts_with("#[") {
            insert_at = previous_start;
            continue;
        }
        break;
    }

    let indent = indentation(text, anchor.line_start);
    Some(vec![(insert_at..insert_at, format!("{indent}public function __construct({parameter}){eol}{indent}{{{eol}{indent}}}{eol}{eol}"))])
}

pub fn rewrites(text: &str, at: usize) -> Vec<Rewrite> {
    let masked = php::mask_comments(text);
    let Some(class) = class_of(&masked) else { return Vec::new() };
    let found = methods(&masked, &class);

    [scope_attribute(text, &masked, &found, at), move_argument(text, &masked, &class, &found, at), inject_facade(text, &masked, &class, &found, at)]
        .into_iter()
        .flatten()
        .collect()
}

fn scope_attribute(text: &str, masked: &str, found: &[Method], at: usize) -> Option<Rewrite> {
    if !masked.contains(" extends Model") {
        return None;
    }

    let method = found.iter().find(|m| m.line_start <= at && at <= m.params.start)?;
    let rest = method.name.strip_prefix("scope").filter(|r| r.chars().next().is_some_and(|c| c.is_ascii_uppercase()))?;
    let new_name = camel(rest);
    if found.iter().any(|m| m.name == new_name) || masked.contains(&format!("{}(", method.name)) && whole_word_positions(masked, &method.name).len() > 1 {
        return None;
    }

    let import = php::insert_use(text, "Illuminate\\Database\\Eloquent\\Attributes\\Scope")?;
    let line = &text[method.line_start..line_end(text, method.line_start)];
    let name_at = method.line_start + line.find("function ")? + "function ".len();
    let indent = indentation(text, method.line_start);
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let visibility_end = method.line_start + line.find("function ")?;
    let modifiers = text[method.line_start..visibility_end].trim().replace("public", "protected");
    let modifiers = if modifiers.is_empty() { "protected".to_owned() } else { modifiers };

    let mut edits = vec![
        (method.line_start..name_at + method.name.len(), format!("{indent}#[{}]{eol}{indent}{modifiers} function {new_name}", import.short_name)),
    ];
    if !import.text.is_empty() {
        edits.insert(0, (import.offset..import.offset, import.text));
    }

    Some(Rewrite { title: format!("Use #[Scope] on {new_name}()"), kind: "refactor.rewrite", edits })
}

fn move_argument(text: &str, masked: &str, class: &Class, found: &[Method], at: usize) -> Option<Rewrite> {
    let method = found.iter().find(|m| m.params.start <= at && at <= m.params.end && m.name != "__construct" && !m.is_static)?;
    let arguments = php::split(masked, method.params.clone());
    let (index, argument) = arguments.iter().enumerate().find(|(_, range)| range.start <= at && at <= range.end)?;
    let source = masked[argument.clone()].trim();

    let (ty, variable) = source.rsplit_once(char::is_whitespace)?;
    let name = variable.strip_prefix('$')?;
    let ty = ty.trim();
    let class_like = ty.trim_start_matches('\\').chars().next().is_some_and(|c| c.is_ascii_uppercase());
    if !class_like || ty.contains(['|', '&', '?', '=']) || ty.split_whitespace().count() > 1 || name.is_empty() || !name.bytes().all(is_identifier) {
        return None;
    }

    let body = &masked[method.body.clone()];
    let variable = format!("${name}");
    let uses = whole_word_positions(body, &variable);
    if body.contains("compact(") || body.contains("extract(") || body.contains("$$") {
        return None;
    }
    for &position in &uses {
        let after = body[position + variable.len()..].trim_start();
        let before = body[..position].trim_end();
        let assigned = after.starts_with('=') && !after.starts_with("==") && !after.starts_with("=>");
        if assigned || before.ends_with(" as") || before.ends_with("static") || before.ends_with("global") {
            return None;
        }
    }
    if body.match_indices("use (").any(|(open, _)| body[open..].find(')').is_some_and(|close| body[open..open + close].contains(&variable))) {
        return None;
    }

    let mut edits = constructor_edits(text, masked, class, found, ty, name)?;
    let removal = if arguments.len() == 1 {
        method.params.clone()
    } else if index == 0 {
        let next = arguments[1].start;
        argument.start..next + (masked[next..].len() - masked[next..].trim_start().len())
    } else {
        arguments[index - 1].end..argument.end
    };
    edits.push((removal, String::new()));
    for position in uses {
        let start = method.body.start + position;
        edits.push((start..start + variable.len(), format!("$this->{name}")));
    }

    Some(Rewrite { title: format!("Move {variable} to a constructor property"), kind: "refactor.rewrite", edits })
}

fn inject_facade(text: &str, masked: &str, class: &Class, found: &[Method], at: usize) -> Option<Rewrite> {
    let bytes = masked.as_bytes();
    let start = (0..at.min(bytes.len())).rev().take_while(|&i| is_identifier(bytes[i])).last().unwrap_or(at);
    let end = (at..bytes.len()).take_while(|&i| is_identifier(bytes[i])).last().map_or(at, |i| i + 1);
    let facade = &masked[start..end];
    let (_, fqcn) = FACADE_CLASSES.iter().find(|(name, _)| *name == facade)?;
    if !masked[end..].starts_with("::") || start > 0 && matches!(bytes[start - 1], b'\\' | b'$' | b':' | b'>') {
        return None;
    }

    let import_line = format!("use Illuminate\\Support\\Facades\\{facade};");
    if !masked.contains(&import_line) {
        return None;
    }

    let mut calls = Vec::new();
    for position in whole_word_positions(&masked[class.open..class.close], facade) {
        let position = class.open + position;
        let before_ok = position == 0 || !matches!(bytes[position - 1], b'\\' | b'$' | b':' | b'>') && !is_identifier(bytes[position - 1]);
        if !before_ok || !masked[position + facade.len()..].starts_with("::") {
            continue;
        }

        let after = &masked[position + facade.len() + 2..];
        let name_length = after.bytes().take_while(|b| is_identifier(*b)).count();
        if name_length == 0 || !after[name_length..].trim_start().starts_with('(') {
            return None;
        }

        let method = found.iter().find(|m| m.body.contains(&position))?;
        if method.is_static {
            return None;
        }
        calls.push(position);
    }
    if calls.is_empty() || masked.matches(&format!("{facade}::")).count() != calls.len() {
        return None;
    }

    let property = camel(facade);
    let import = php::insert_use(text, fqcn)?;
    let ty = import.short_name.clone();
    let mut edits = constructor_edits(text, masked, class, found, &ty, &property)?;
    if !import.text.is_empty() {
        edits.push((import.offset..import.offset, import.text));
    }

    for position in calls {
        edits.push((position..position + facade.len() + 2, format!("$this->{property}->")));
    }

    let import_at = masked.find(&import_line)?;
    let line_start = masked[..import_at].rfind('\n').map_or(0, |i| i + 1);
    edits.push((line_start..line_end(text, import_at), String::new()));

    Some(Rewrite { title: format!("Inject {ty} instead of the {facade} facade"), kind: "refactor.rewrite", edits })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, rewrite: &Rewrite) -> String {
        let mut edits = rewrite.edits.clone();
        edits.sort_by_key(|(range, _)| std::cmp::Reverse((range.start, range.end)));
        let mut out = text.to_owned();
        for (range, replacement) in edits {
            out.replace_range(range, &replacement);
        }
        out
    }

    fn at(text: &str, marker: &str) -> usize {
        text.find(marker).unwrap() + 1
    }

    #[test]
    fn scope_method_becomes_an_attribute() {
        let text = "<?php\nnamespace App\\Models;\n\nuse Illuminate\\Database\\Eloquent\\Model;\n\nclass Post extends Model\n{\n    public function scopeActive($query)\n    {\n        return $query->where('a', 1);\n    }\n}\n";
        let found = rewrites(text, at(text, "scopeActive"));
        let rewrite = found.iter().find(|r| r.title.starts_with("Use #[Scope]")).unwrap();
        let out = apply(text, rewrite);
        assert!(out.contains("use Illuminate\\Database\\Eloquent\\Attributes\\Scope;"), "{out}");
        assert!(out.contains("    #[Scope]\n    protected function active($query)"), "{out}");
    }

    #[test]
    fn scope_is_left_alone_when_called_by_name_or_not_a_model() {
        let text = "<?php\nclass Post extends Model\n{\n    public function scopeActive($q) {}\n    public function a() { return $this->scopeActive(1); }\n}\n";
        assert!(rewrites(text, at(text, "scopeActive")).is_empty());
        let plain = "<?php\nclass Post\n{\n    public function scopeActive($q) {}\n}\n";
        assert!(rewrites(plain, at(plain, "scopeActive")).is_empty());
    }

    #[test]
    fn argument_moves_into_a_new_constructor() {
        let text = "<?php\nclass ReportController extends Controller\n{\n    public function index(Request $request, ReportService $service)\n    {\n        return $service->all();\n    }\n}\n";
        let found = rewrites(text, at(text, "ReportService"));
        let out = apply(text, found.iter().find(|r| r.title.starts_with("Move $service")).unwrap());
        assert_eq!(
            out,
            "<?php\nclass ReportController extends Controller\n{\n    public function __construct(private ReportService $service)\n    {\n    }\n\n    public function index(Request $request)\n    {\n        return $this->service->all();\n    }\n}\n"
        );
    }

    #[test]
    fn argument_joins_an_existing_constructor() {
        let text = "<?php\nclass A\n{\n    public function __construct(private Foo $foo)\n    {\n    }\n\n    public function run(Bar $bar)\n    {\n        $bar->go();\n    }\n}\n";
        let out = apply(text, rewrites(text, at(text, "Bar $bar")).iter().find(|r| r.title.starts_with("Move")).unwrap());
        assert!(out.contains("__construct(private Foo $foo, private Bar $bar)"), "{out}");
        assert!(out.contains("public function run()\n"), "{out}");
        assert!(out.contains("$this->bar->go();"), "{out}");
    }

    #[test]
    fn argument_stays_put_when_reassigned_or_class_has_a_foreign_parent() {
        let reassigned = "<?php\nclass A\n{\n    public function run(Bar $bar)\n    {\n        $bar = new Bar();\n    }\n}\n";
        assert!(rewrites(reassigned, at(reassigned, "Bar $bar")).is_empty());
        let parent = "<?php\nclass A extends Base\n{\n    public function run(Bar $bar)\n    {\n        $bar->go();\n    }\n}\n";
        assert!(rewrites(parent, at(parent, "Bar $bar")).is_empty());
        let scalar = "<?php\nclass A\n{\n    public function run(int $n, ?Bar $bar = null) {}\n}\n";
        assert!(rewrites(scalar, at(scalar, "int $n")).is_empty() && rewrites(scalar, at(scalar, "Bar $bar")).is_empty());
    }

    #[test]
    fn facade_becomes_an_injected_property() {
        let text = "<?php\nnamespace App;\n\nuse Illuminate\\Support\\Facades\\Cache;\n\nclass A\n{\n    public function run()\n    {\n        return Cache::get('x');\n    }\n}\n";
        let found = rewrites(text, at(text, "Cache::get"));
        let out = apply(text, found.iter().find(|r| r.title.starts_with("Inject")).unwrap());
        assert!(out.contains("use Illuminate\\Cache\\CacheManager;"), "{out}");
        assert!(!out.contains("Facades\\Cache"), "{out}");
        assert!(out.contains("public function __construct(private CacheManager $cache)"), "{out}");
        assert!(out.contains("return $this->cache->get('x');"), "{out}");
    }

    #[test]
    fn facade_in_a_static_method_or_used_as_a_constant_is_left_alone() {
        let static_call = "<?php\nuse Illuminate\\Support\\Facades\\Cache;\nclass A\n{\n    public static function run()\n    {\n        return Cache::get('x');\n    }\n}\n";
        assert!(rewrites(static_call, at(static_call, "Cache::get")).iter().all(|r| !r.title.starts_with("Inject")));
        let constant = "<?php\nuse Illuminate\\Support\\Facades\\Cache;\nclass A\n{\n    public function run()\n    {\n        return [Cache::get('x'), Cache::class];\n    }\n}\n";
        assert!(rewrites(constant, at(constant, "Cache::get")).iter().all(|r| !r.title.starts_with("Inject")));
    }
}
