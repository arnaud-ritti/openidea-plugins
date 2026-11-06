//! Class rewrites that need several edits: Doctrine `'App:Product'` aliases,
//! a console command turned invokable, Twig extension methods turned into
//! attributes. Each is offered only when every part of the class is understood;
//! otherwise nothing is offered.

use std::ops::Range;

use crate::checks;
use crate::intentions::{eol_of, find_class, import_for, indent_at, line_start, methods_of, Action, Edit, Method};
use crate::php_checks::{arguments, trimmed};
use crate::php_text::{self, attributes, class_model, mask, matching_close, read_word, skip_whitespace, ClassModel, Target};
use crate::refs;

const CONSOLE_COMMAND: &str = "Symfony\\Component\\Console\\Command\\Command";
const AS_COMMAND: &str = "Symfony\\Component\\Console\\Attribute\\AsCommand";
const ARGUMENT_ATTRIBUTE: &str = "Symfony\\Component\\Console\\Attribute\\Argument";
const OPTION_ATTRIBUTE: &str = "Symfony\\Component\\Console\\Attribute\\Option";
const ABSTRACT_EXTENSION: &str = "Twig\\Extension\\AbstractExtension";
const ENTITY_MANAGER_CALLS: [&str; 5] = ["->getRepository(", "->getReference(", "->getClassMetadata(", "->getPartialReference(", "->find("];
const COMMAND_LIFECYCLE: [&str; 5] = ["initialize", "interact", "complete", "run", "setName"];
const UNCONVERTIBLE_EXTENSION_METHODS: [&str; 5] = ["getGlobals", "getOperators", "getNodeVisitors", "getTokenParsers", "getName"];
const TWIG_KINDS: [(&str, &str, &str, &str); 3] = [
    ("getFilters", "TwigFilter", "AsTwigFilter", "Twig\\Attribute\\AsTwigFilter"),
    ("getFunctions", "TwigFunction", "AsTwigFunction", "Twig\\Attribute\\AsTwigFunction"),
    ("getTests", "TwigTest", "AsTwigTest", "Twig\\Attribute\\AsTwigTest"),
];

pub fn php_rewrites(text: &str, at: usize, known_class: &dyn Fn(&str) -> bool) -> Vec<Action> {
    let masked = mask(text);
    let mut actions = Vec::new();
    actions.extend(doctrine_alias(text, at, known_class));

    let (Some(model), Some(class)) = (class_model(text), find_class(&masked)) else { return actions };
    let methods = methods_of(text, &masked, &class);
    actions.extend(invokable_command(text, &masked, at, &model, &methods));
    actions.extend(twig_attributes(text, &masked, at, &model, &methods));
    actions.extend(property_to_constructor(text, &masked, at, &model));
    actions
}

/// A typed, uninitialised class-typed property moved into the constructor as a promoted parameter.
fn property_to_constructor(text: &str, masked: &str, at: usize, model: &ClassModel) -> Option<Action> {
    let line = line_start(text, at);
    let end = masked[line..].find('\n').map_or(masked.len(), |found| line + found + 1);
    let declaration = masked[line..end].trim();
    let inside_methods = model.methods.iter().any(|method| method.body.as_ref().is_some_and(|body| body.contains(&at)) || method.parameters.contains(&at));
    if inside_methods || !model.body.contains(&at) || !declaration.ends_with(';') || declaration.contains(['=', '(', '|', '?']) {
        return None;
    }

    let words: Vec<&str> = declaration.trim_end_matches(';').split_whitespace().collect();
    let (visibility, rest) = words.split_first()?;
    let rest: Vec<&str> = rest.iter().copied().filter(|word| *word != "readonly").collect();
    let [type_name, variable] = rest[..] else { return None };
    let readonly = words.contains(&"readonly");
    let name = variable.strip_prefix('$')?;
    let is_class_type = type_name.starts_with(|c: char| c.is_ascii_uppercase() || c == '\\') && !php_text::BUILT_IN_TYPES.contains(&type_name.to_ascii_lowercase().as_str());
    if !matches!(*visibility, "private" | "protected" | "public") || !is_class_type {
        return None;
    }

    let previous_line = text[..line].trim_end().lines().last().unwrap_or_default().trim();
    if previous_line.starts_with("#[") || previous_line.ends_with("*/") || previous_line.ends_with(']') {
        return None;
    }

    let promoted = format!("{visibility} {}{type_name} ${name}", if readonly { "readonly " } else { "" });
    let eol = eol_of(text);
    let indent = indent_at(text, at);

    match model.method("__construct") {
        Some(constructor) => {
            let body = constructor.body.clone()?;
            if masked[body].contains(&format!("$this->{name}")) {
                return None;
            }

            let close = constructor.parameters.end - 1;
            let before_close = masked[constructor.parameters.start + 1..close].trim_end();
            let last = constructor.parameters.start + 1 + before_close.len();
            let edit = match (before_close.is_empty(), masked[constructor.parameters.clone()].contains('\n')) {
                (true, _) => (close..close, promoted),
                (false, false) => (last..last, format!(", {promoted}")),
                (false, true) => {
                    let parameter_indent = indent_at(text, constructor.parameters.start + 1 + before_close.rfind('\n').map_or(0, |found| found + 1) + 1).to_owned();
                    let trailing_comma = before_close.ends_with(',');
                    match trailing_comma {
                        true => (last..last, format!("{eol}{parameter_indent}{promoted},")),
                        false => (last..last, format!(",{eol}{parameter_indent}{promoted}")),
                    }
                }
            };
            Some(Action { title: format!("Move ${name} to the constructor"), kind: "refactor.rewrite", edits: vec![(line..end, String::new()), edit] })
        }
        None => {
            let constructor = format!("{indent}public function __construct({promoted})\n{indent}{{\n{indent}}}\n").replace('\n', eol);
            Some(Action { title: format!("Move ${name} to a new constructor"), kind: "refactor.rewrite", edits: vec![(line..end, constructor)] })
        }
    }
}

fn literal_text(text: &str, range: Range<usize>) -> Option<&str> {
    let raw = text[range].trim();
    let quote = raw.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let inner = raw.strip_prefix(quote)?.strip_suffix(quote)?;
    (!inner.contains([quote, '\\', '$', '{'])).then_some(inner)
}

fn literal_source(text: &str, masked: &str, range: Range<usize>) -> Option<String> {
    literal_text(text, range.clone())?;
    Some(text[trimmed(masked, range)].to_owned())
}

fn compact(text: &str, range: Range<usize>) -> String {
    text[range].chars().filter(|c| !c.is_whitespace()).collect()
}

fn doctrine_alias(text: &str, at: usize, known_class: &dyn Fn(&str) -> bool) -> Option<Action> {
    for needle in ENTITY_MANAGER_CALLS {
        for open in checks::call_opens(text, needle) {
            let Some(first) = refs::arguments(text, open).into_iter().next() else { continue };
            let Some(inner) = checks::literal(text, first) else { continue };
            let outer = inner.start - 1..inner.end + 1;
            if !(outer.start <= at && at <= outer.end) {
                continue;
            }

            let Some((alias, entity)) = text[inner].split_once(':') else { continue };
            let alias_ok = !alias.is_empty() && alias.bytes().all(|b| refs::is_identifier(b) || b == b'\\');
            let entity_ok = !entity.is_empty() && entity.bytes().all(|b| refs::is_identifier(b) || b == b'\\');
            let fqcn = format!("{alias}\\Entity\\{entity}");
            if !alias_ok || !entity_ok || !known_class(&fqcn) {
                return None;
            }

            let (import, short) = import_for(text, &fqcn)?;
            let replacement = format!("{}::class", short.rsplit('\\').next().unwrap_or(&entity));
            return Some(Action {
                title: format!("Use {replacement}"),
                kind: "refactor.rewrite",
                edits: import.into_iter().chain([(outer, replacement)]).collect(),
            });
        }
    }

    None
}

/// The range to delete for a whole member: its lines and one blank line after it.
fn removal(text: &str, span: &Range<usize>) -> Range<usize> {
    let start = line_start(text, span.start);
    let mut end = text[span.end..].find('\n').map_or(text.len(), |found| span.end + found + 1);
    let rest = &text[end..];
    let blank = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    if rest[blank..].starts_with('\n') || rest[blank..].starts_with("\r\n") {
        end += blank + rest[blank..].find('\n').map_or(0, |found| found + 1);
    }

    start..end
}

fn camel(name: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in name.chars() {
        match c {
            '-' | '_' => upper = true,
            c if upper => {
                out.push(c.to_ascii_uppercase());
                upper = false;
            }
            c => out.push(c),
        }
    }
    out
}

struct Input {
    attribute: &'static str,
    attribute_fqcn: &'static str,
    parameter: String,
}

/// `addArgument`/`addOption` calls of a `configure()` body as invokable parameters;
/// `None` for any other statement or an argument shape this cannot express.
fn command_parameters(text: &str, masked: &str, body: Range<usize>) -> Option<Vec<Input>> {
    let mut inputs = Vec::new();
    let mut at = skip_whitespace(masked, body.start);

    while at < body.end {
        if !masked[at..body.end].starts_with("$this") {
            return None;
        }
        at = skip_whitespace(masked, at + "$this".len());

        loop {
            if !masked[at..body.end].starts_with("->") {
                return None;
            }

            let word = read_word(masked, skip_whitespace(masked, at + 2));
            let open = skip_whitespace(masked, word.end);
            if !masked[open..body.end].starts_with('(') {
                return None;
            }

            let close = matching_close(masked, open)?;
            let list = arguments(masked, open);
            inputs.push(match &masked[word] {
                "addArgument" => argument_parameter(text, masked, &list)?,
                "addOption" => option_parameter(text, masked, &list)?,
                _ => return None,
            });

            at = skip_whitespace(masked, close + 1);
            if masked[at..body.end].starts_with(';') {
                at = skip_whitespace(masked, at + 1);
                break;
            }
        }
    }

    Some(inputs)
}

fn input_name(text: &str, list: &[Range<usize>]) -> Option<(String, String)> {
    let name = literal_text(text, list.first()?.clone())?;
    let valid = !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'));
    valid.then(|| (name.to_owned(), camel(name)))
}

fn attribute_arguments(name: &str, parameter: &str, shortcut: Option<&str>, description: Option<&str>) -> String {
    let mut parts = Vec::new();
    if name != parameter {
        parts.push(format!("name: '{name}'"));
    }
    parts.extend(shortcut.map(|shortcut| format!("shortcut: {shortcut}")));
    parts.extend(description.map(|description| format!("description: {description}")));

    match parts.is_empty() {
        true => String::new(),
        false => format!("({})", parts.join(", ")),
    }
}

fn argument_parameter(text: &str, masked: &str, list: &[Range<usize>]) -> Option<Input> {
    if list.len() > 3 {
        return None;
    }

    let (name, parameter) = input_name(text, list)?;
    let optional = match list.get(1).map(|mode| compact(text, mode.clone())) {
        None => true,
        Some(mode) if mode.ends_with("InputArgument::REQUIRED") => false,
        Some(mode) if mode.ends_with("InputArgument::OPTIONAL") => true,
        Some(_) => return None,
    };

    let description = match list.get(2) {
        Some(range) => Some(literal_source(text, masked, range.clone())?),
        None => None,
    };
    let declaration = if optional { format!("?string ${parameter} = null") } else { format!("string ${parameter}") };
    let arguments = attribute_arguments(&name, &parameter, None, description.as_deref());

    Some(Input { attribute: "Argument", attribute_fqcn: ARGUMENT_ATTRIBUTE, parameter: format!("#[Argument{arguments}] {declaration}") })
}

fn option_parameter(text: &str, masked: &str, list: &[Range<usize>]) -> Option<Input> {
    if list.len() > 4 {
        return None;
    }

    let (name, parameter) = input_name(text, list)?;
    let shortcut = match list.get(1).map(|shortcut| compact(text, shortcut.clone())) {
        None => None,
        Some(shortcut) if shortcut == "null" => None,
        Some(_) => Some(literal_source(text, masked, list[1].clone())?),
    };
    let flag = match list.get(2).map(|mode| compact(text, mode.clone())) {
        None => true,
        Some(mode) if mode.ends_with("InputOption::VALUE_NONE") => true,
        Some(mode) if mode.ends_with("InputOption::VALUE_REQUIRED") => false,
        Some(_) => return None,
    };

    let description = match list.get(3) {
        Some(range) => Some(literal_source(text, masked, range.clone())?),
        None => None,
    };
    let declaration = if flag { format!("bool ${parameter} = false") } else { format!("?string ${parameter} = null") };
    let arguments = attribute_arguments(&name, &parameter, shortcut.as_deref(), description.as_deref());

    Some(Input { attribute: "Option", attribute_fqcn: OPTION_ATTRIBUTE, parameter: format!("#[Option{arguments}] {declaration}") })
}

/// Whether the body only calls own methods and reads own properties of `$this`.
fn uses_only_own_members(masked: &str, body: Range<usize>, model: &ClassModel, methods: &[Method]) -> bool {
    masked[body.clone()].match_indices("$this").all(|(found, _)| {
        let at = body.start + found + "$this".len();
        let rest = &masked[at..body.end];
        if !rest.starts_with("->") {
            return false;
        }

        let name = read_word(masked, at + 2);
        let call = masked[name.end..].trim_start().starts_with('(');
        let word = &masked[name];
        match call {
            true => methods.iter().any(|method| method.name == word),
            false => masked[model.body.clone()].contains(&format!("${word}")),
        }
    })
}

fn invokable_command(text: &str, masked: &str, at: usize, model: &ClassModel, methods: &[Method]) -> Option<Action> {
    if model.parent.as_deref() != Some(CONSOLE_COMMAND) || model.has_traits {
        return None;
    }

    let has_command_attribute = attributes(text).iter().any(|attribute| attribute.name == AS_COMMAND && matches!(attribute.target, Target::Class { .. }));
    let execute = model.method("execute")?;
    let signature = &text[execute.parameters.clone()];
    let signature_compact: String = signature.chars().filter(|c| !c.is_whitespace()).collect();
    let returns_int = execute.return_type.as_deref() == Some("int");
    let standard_signature = signature_compact.ends_with("InputInterface$input,OutputInterface$output)") || signature_compact.ends_with("InputInterface$input,OutputInterface$output,)");
    if !has_command_attribute || !returns_int || !standard_signature || execute.params.len() != 2 {
        return None;
    }

    if model.methods.iter().any(|method| COMMAND_LIFECYCLE.contains(&method.name.as_str())) {
        return None;
    }

    let execute_method = methods.iter().find(|method| method.name == "execute")?;
    let configure_method = methods.iter().find(|method| method.name == "configure");
    let constructor = methods.iter().find(|method| method.name == "__construct");
    let touches_method = |method: &Method| method.span.contains(&at) || method.span.end == at || method.span.start == at;
    let header = line_start(text, model.name_range.start)..text[model.name_range.start..].find('\n').map_or(text.len(), |found| model.name_range.start + found);
    let on_header = header.contains(&at) || header.end == at;
    if !(on_header || touches_method(execute_method) || configure_method.is_some_and(touches_method)) {
        return None;
    }

    let execute_body = {
        let open = masked[execute_method.parameters.end..].find('{')? + execute_method.parameters.end;
        open + 1..matching_close(masked, open)?
    };
    if !uses_only_own_members(masked, execute_body, model, methods) {
        return None;
    }

    let inputs = match configure_method {
        Some(configure) => {
            let open = masked[configure.parameters.end..].find('{')? + configure.parameters.end;
            command_parameters(text, masked, open + 1..matching_close(masked, open)?)?
        }
        None => Vec::new(),
    };

    let mut edits: Vec<Edit> = Vec::new();
    let parent_range = model.parent_range.clone()?;
    let before_parent = masked[..parent_range.start].trim_end();
    let without_keyword = before_parent.strip_suffix("extends")?.trim_end();
    edits.push((without_keyword.len()..parent_range.end, String::new()));

    if let Some(configure) = configure_method {
        edits.push((removal(text, &configure.span), String::new()));
    }

    if let Some(constructor) = constructor {
        let open = masked[constructor.parameters.end..].find('{')? + constructor.parameters.end;
        let body = open + 1..matching_close(masked, open)?;
        let call = masked[body.clone()].find("parent::__construct")? + body.start;
        let after = skip_whitespace(masked, call + "parent::__construct".len());
        let empty = masked[after..].starts_with('(') && masked[skip_whitespace(masked, after + 1)..].starts_with(");");
        if !empty || masked[body.clone()].matches("parent::__construct").count() != 1 {
            return None;
        }

        let end = skip_whitespace(masked, after + 1) + 2;
        let line = line_start(text, call);
        let whole_line = text[line..call].trim().is_empty() && text[end..].lines().next().is_some_and(|rest| rest.trim().is_empty());
        if !whole_line {
            return None;
        }
        edits.push((line..text[end..].find('\n').map_or(text.len(), |found| end + found + 1), String::new()));
    }

    let eol = eol_of(text);
    let indent = indent_at(text, execute_method.span.start);
    let leading = &text[execute_method.span.start..execute_method.name_range.start];
    let leading = leading.replace("protected", "public").replace("private", "public");

    let mut parameter_lines: Vec<String> = refs::arguments(text, execute.parameters.start).into_iter().map(|part| part.trim().to_owned()).filter(|part| !part.is_empty()).collect();
    let mut imports: Vec<Edit> = Vec::new();
    for input in &inputs {
        parameter_lines.push(input.parameter.clone());
        if !imports.iter().any(|(_, line)| line.contains(input.attribute)) {
            let (import, short) = import_for(text, input.attribute_fqcn)?;
            if short != input.attribute {
                return None;
            }
            imports.extend(import);
        }
    }

    let list = if inputs.is_empty() {
        parameter_lines.join(", ")
    } else {
        let lines: String = parameter_lines.iter().map(|line| format!("{indent}    {line},{eol}")).collect();
        format!("{eol}{lines}{indent}")
    };
    let header = format!("{leading}__invoke({list})");
    let close = execute.parameters.end;
    edits.push((execute_method.span.start..close, header));
    edits.extend(imports);

    Some(Action { title: "Convert to invokable command".into(), kind: "refactor.rewrite", edits })
}

/// `new TwigFilter('name', [$this, 'method'])` entries of a `return [...]` body: (name literal, method).
fn twig_entries(text: &str, masked: &str, method: &Method, class_name: &str) -> Option<Vec<(String, String)>> {
    let open = masked[method.parameters.end..].find('{')? + method.parameters.end;
    let close = matching_close(masked, open)?;
    let ret = masked[open..close].find("return")? + open;
    let bracket = masked[ret..close].find('[')? + ret;
    let end = matching_close(masked, bracket)?;
    let tail_ok = masked[end + 1..close].trim().trim_start_matches(';').trim().is_empty() && masked[open + 1..ret].trim().is_empty();
    if !tail_ok {
        return None;
    }

    let mut entries = Vec::new();
    for item in arguments(masked, bracket) {
        let item = trimmed(masked, item);
        let prefix = format!("new {class_name}(");
        if !masked[item.clone()].starts_with(&prefix) {
            return None;
        }

        let parenthesis = item.start + prefix.len() - 1;
        let list = arguments(masked, parenthesis);
        if list.len() != 2 || matching_close(masked, parenthesis)? + 1 != item.end {
            return None;
        }

        literal_text(text, list[0].clone())?;
        let callable = compact(text, list[1].clone());
        let target = callable.strip_prefix("[$this,")?.strip_suffix(']')?;
        let target = target.trim_matches(['\'', '"']);
        if target.is_empty() || !target.bytes().all(refs::is_identifier) || target.len() + 2 != callable.len() - "[$this,]".len() {
            return None;
        }

        entries.push((text[trimmed(masked, list[0].clone())].to_owned(), target.to_owned()));
    }

    Some(entries)
}

fn twig_attributes(text: &str, masked: &str, at: usize, model: &ClassModel, methods: &[Method]) -> Option<Action> {
    if model.parent.as_deref() != Some(ABSTRACT_EXTENSION) || model.has_traits {
        return None;
    }

    if model.methods.iter().any(|method| UNCONVERTIBLE_EXTENSION_METHODS.contains(&method.name.as_str())) {
        return None;
    }

    let converting: Vec<&Method> = methods.iter().filter(|method| TWIG_KINDS.iter().any(|(name, ..)| *name == method.name)).collect();
    if converting.is_empty() || !converting.iter().any(|method| method.span.contains(&at) || method.span.end == at) {
        return None;
    }

    let mut inserts: Vec<(usize, String)> = Vec::new();
    let mut edits: Vec<Edit> = Vec::new();
    let mut imports: Vec<Edit> = Vec::new();
    let eol = eol_of(text);

    for method in &converting {
        let (_, class_name, attribute, attribute_fqcn) = TWIG_KINDS.iter().find(|(name, ..)| *name == method.name)?;
        if !text.contains(&format!("Twig\\{class_name}")) {
            return None;
        }

        let entries = twig_entries(text, masked, method, class_name)?;
        let (import, short) = import_for(text, attribute_fqcn)?;
        if short != *attribute {
            return None;
        }
        imports.extend(import);

        for (name, target) in entries {
            let target_method = methods.iter().find(|candidate| candidate.name == target && candidate.visibility == php_text::Visibility::Public)?;
            let indent = indent_at(text, target_method.span.start);
            inserts.push((target_method.span.start, format!("#[{attribute}({name})]{eol}{indent}")));
        }
        edits.push((removal(text, &method.span), String::new()));
    }

    inserts.sort_by_key(|(offset, _)| *offset);
    let mut merged: Vec<(usize, String)> = Vec::new();
    for (offset, line) in inserts {
        match merged.last_mut() {
            Some((last, text)) if *last == offset => text.push_str(&line),
            _ => merged.push((offset, line)),
        }
    }
    edits.extend(merged.into_iter().map(|(offset, line)| (offset..offset, line)));
    imports.dedup();
    edits.extend(imports);

    Some(Action { title: "Convert Twig extension methods to attributes".into(), kind: "refactor.rewrite", edits })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, action: &Action) -> String {
        let mut edits: Vec<&Edit> = action.edits.iter().collect();
        edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        let mut out = text.to_owned();
        for (range, replacement) in edits {
            out.replace_range(range.clone(), replacement);
        }
        out
    }

    fn known(class: &str) -> bool {
        class == "App\\Entity\\Product"
    }

    fn rewrites(text: &str, marker: &str) -> Vec<Action> {
        php_rewrites(text, text.find(marker).unwrap(), &known)
    }

    const REPOSITORY: &str = "<?php\nnamespace App\\Service;\n\nclass Shop\n{\n    public function find($em)\n    {\n        return $em->getRepository('App:Product')->findAll();\n    }\n}\n";

    #[test]
    fn doctrine_alias_becomes_a_class_constant_with_import() {
        let actions = rewrites(REPOSITORY, "App:Product");
        assert_eq!(actions[0].title, "Use Product::class");
        let out = apply(REPOSITORY, &actions[0]);
        assert!(out.contains("getRepository(Product::class)"), "{out}");
        assert!(out.contains("use App\\Entity\\Product;"), "{out}");
    }

    #[test]
    fn doctrine_alias_is_quiet_for_unknown_entities_and_away_from_the_literal() {
        assert!(rewrites(&REPOSITORY.replace("Product", "Gone"), "App:Gone").is_empty());
        assert!(rewrites(REPOSITORY, "findAll").is_empty());
        assert!(rewrites(&REPOSITORY.replace("'App:Product'", "$alias"), "$alias").is_empty());
        assert!(rewrites(&REPOSITORY.replace("'App:Product'", "'Product'"), "'Product'").is_empty());
    }

    const COMMAND: &str = "<?php\nnamespace App\\Command;\n\nuse Symfony\\Component\\Console\\Attribute\\AsCommand;\nuse Symfony\\Component\\Console\\Command\\Command;\nuse Symfony\\Component\\Console\\Input\\InputArgument;\nuse Symfony\\Component\\Console\\Input\\InputInterface;\nuse Symfony\\Component\\Console\\Input\\InputOption;\nuse Symfony\\Component\\Console\\Output\\OutputInterface;\n\n#[AsCommand(name: 'app:sync')]\nclass SyncCommand extends Command\n{\n    protected function configure(): void\n    {\n        $this\n            ->addArgument('target', InputArgument::REQUIRED, 'Where to sync')\n            ->addArgument('mode')\n            ->addOption('dry-run', 'd', InputOption::VALUE_NONE, 'Do nothing')\n            ->addOption('limit', null, InputOption::VALUE_REQUIRED)\n        ;\n    }\n\n    protected function execute(InputInterface $input, OutputInterface $output): int\n    {\n        $output->writeln($input->getArgument('target'));\n\n        return Command::SUCCESS;\n    }\n}\n";

    #[test]
    fn command_becomes_invokable_with_attribute_parameters() {
        let actions = rewrites(COMMAND, "execute(");
        let action = actions.iter().find(|action| action.title == "Convert to invokable command").unwrap();
        let out = apply(COMMAND, action);
        let expected = "#[AsCommand(name: 'app:sync')]\nclass SyncCommand\n{\n    public function __invoke(\n        InputInterface $input,\n        OutputInterface $output,\n        #[Argument(description: 'Where to sync')] string $target,\n        #[Argument] ?string $mode = null,\n        #[Option(name: 'dry-run', shortcut: 'd', description: 'Do nothing')] bool $dryRun = false,\n        #[Option] ?string $limit = null,\n    ): int\n    {\n        $output->writeln($input->getArgument('target'));\n";
        assert!(out.contains(expected), "{out}");
        assert!(!out.contains("configure"), "{out}");
        assert!(out.contains("use Symfony\\Component\\Console\\Attribute\\Argument;") && out.contains("use Symfony\\Component\\Console\\Attribute\\Option;"), "{out}");
    }

    #[test]
    fn command_without_configure_keeps_a_one_line_signature_and_drops_the_parent_constructor_call() {
        let text = "<?php\nnamespace App\\Command;\n\nuse Symfony\\Component\\Console\\Attribute\\AsCommand;\nuse Symfony\\Component\\Console\\Command\\Command;\nuse Symfony\\Component\\Console\\Input\\InputInterface;\nuse Symfony\\Component\\Console\\Output\\OutputInterface;\n\n#[AsCommand(name: 'app:x')]\nclass XCommand extends Command\n{\n    public function __construct(private Service $service)\n    {\n        parent::__construct();\n    }\n\n    protected function execute(InputInterface $input, OutputInterface $output): int\n    {\n        $this->service->run();\n\n        return self::SUCCESS;\n    }\n}\n";
        let actions = rewrites(text, "class XCommand");
        let out = apply(text, actions.iter().find(|action| action.title == "Convert to invokable command").unwrap());
        assert!(out.contains("class XCommand\n{\n    public function __construct(private Service $service)\n    {\n    }\n"), "{out}");
        assert!(out.contains("public function __invoke(InputInterface $input, OutputInterface $output): int"), "{out}");
    }

    #[test]
    fn command_rewrite_is_not_offered_when_something_is_not_understood() {
        let cases = [
            COMMAND.replace("->addOption('limit', null, InputOption::VALUE_REQUIRED)", "->setDescription('x')"),
            COMMAND.replace("InputOption::VALUE_REQUIRED", "InputOption::VALUE_REQUIRED | InputOption::VALUE_IS_ARRAY"),
            COMMAND.replace("InputOption::VALUE_NONE", "InputOption::VALUE_OPTIONAL"),
            COMMAND.replace("'Where to sync'", "$desc"),
            COMMAND.replace("$output->writeln(", "$this->getHelper('x');$output->writeln("),
            COMMAND.replace("#[AsCommand(name: 'app:sync')]\n", ""),
            COMMAND.replace("OutputInterface $output): int", "OutputInterface $output, int $extra): int"),
            COMMAND.replace("protected function configure(): void", "protected function initialize(): void {}\n\n    protected function configure(): void"),
            COMMAND.replace("extends Command", "extends Other"),
            COMMAND.replace("addArgument('mode')", "addArgument('mode', InputArgument::OPTIONAL, 'd', 'default')"),
            COMMAND.replace("'target'", "'we ird'"),
        ];
        for text in cases {
            let actions = rewrites(&text, "execute(");
            assert!(!actions.iter().any(|action| action.title == "Convert to invokable command"), "{text}");
        }
    }

    #[test]
    fn command_rewrite_is_quiet_away_from_the_command_members() {
        let at = COMMAND.find("InputOption;").unwrap();
        assert!(php_rewrites(COMMAND, at, &known).is_empty());
    }

    const EXTENSION: &str = "<?php\nnamespace App\\Twig;\n\nuse Twig\\Extension\\AbstractExtension;\nuse Twig\\TwigFilter;\nuse Twig\\TwigFunction;\n\nclass AppExtension extends AbstractExtension\n{\n    public function getFilters(): array\n    {\n        return [\n            new TwigFilter('price', [$this, 'formatPrice']),\n        ];\n    }\n\n    public function getFunctions(): array\n    {\n        return [\n            new TwigFunction('price_of', [$this, 'formatPrice']),\n            new TwigFunction(\"area\", [$this, 'area']),\n        ];\n    }\n\n    public function formatPrice(int $n): string\n    {\n        return '';\n    }\n\n    public function area(): int\n    {\n        return 1;\n    }\n}\n";

    #[test]
    fn twig_extension_methods_become_attributes() {
        let actions = rewrites(EXTENSION, "getFilters");
        let out = apply(EXTENSION, actions.iter().find(|action| action.title == "Convert Twig extension methods to attributes").unwrap());
        assert!(!out.contains("getFilters") && !out.contains("getFunctions"), "{out}");
        assert!(out.contains("    #[AsTwigFilter('price')]\n    #[AsTwigFunction('price_of')]\n    public function formatPrice(int $n): string"), "{out}");
        assert!(out.contains("    #[AsTwigFunction(\"area\")]\n    public function area(): int"), "{out}");
        assert!(out.contains("use Twig\\Attribute\\AsTwigFilter;") && out.contains("use Twig\\Attribute\\AsTwigFunction;"), "{out}");
        assert!(out.contains("class AppExtension extends AbstractExtension\n{\n    #[AsTwigFilter('price')]"), "{out}");
    }

    #[test]
    fn twig_extension_rewrite_is_not_offered_for_options_unknown_callables_or_other_methods() {
        let cases = [
            EXTENSION.replace("[$this, 'formatPrice']),\n        ];\n    }\n\n    public function getFunctions", "[$this, 'formatPrice'], ['is_safe' => ['html']]),\n        ];\n    }\n\n    public function getFunctions"),
            EXTENSION.replace("[$this, 'area']", "'strtoupper'"),
            EXTENSION.replace("[$this, 'area']", "[$this, 'gone']"),
            EXTENSION.replace("public function area()", "public function getGlobals()"),
            EXTENSION.replace("return [\n            new TwigFilter('price', [$this, 'formatPrice']),\n        ];", "$a = 1;\n        return [new TwigFilter('price', [$this, 'formatPrice'])];"),
            EXTENSION.replace("extends AbstractExtension", "extends Other"),
            EXTENSION.replace("use Twig\\TwigFilter;\n", ""),
        ];
        for text in cases {
            let actions = rewrites(&text, "getFilters");
            assert!(!actions.iter().any(|action| action.title.starts_with("Convert Twig")), "{text}");
        }
    }

    #[test]
    fn rewrites_survive_crlf_multibyte_and_degenerate_text() {
        let crlf = EXTENSION.replace('\n', "\r\n");
        let out = apply(&crlf, rewrites(&crlf, "getFilters").iter().find(|action| action.title.starts_with("Convert Twig")).unwrap());
        assert!(out.contains("    #[AsTwigFilter('price')]\r\n    #[AsTwigFunction('price_of')]\r\n    public function formatPrice"), "{out:?}");

        let multibyte = COMMAND.replace("'Where to sync'", "'Où synchroniser é😀'");
        let out = apply(&multibyte, rewrites(&multibyte, "execute(").iter().find(|action| action.title.starts_with("Convert to")).unwrap());
        assert!(out.contains("description: 'Où synchroniser é😀'"), "{out}");

        for text in ["<?php\nclass A extends Command { function execute(", "<?php\n->getRepository('a:", "<?php\nclass X extends AbstractExtension { function getFilters() { return [new", ""] {
            php_rewrites(text, text.len(), &known);
            php_rewrites(text, 0, &known);
        }
    }

    #[test]
    fn property_moves_into_an_existing_or_new_constructor() {
        let with = "<?php\nclass A\n{\n    private Logger $logger;\n\n    public function __construct(private Mailer $mailer)\n    {\n    }\n}\n";
        let at = with.find("$logger").unwrap();
        let actions = php_rewrites(with, at, &|_| false);
        let action = actions.iter().find(|action| action.title == "Move $logger to the constructor").unwrap();
        assert_eq!(apply(with, action), "<?php\nclass A\n{\n\n    public function __construct(private Mailer $mailer, private Logger $logger)\n    {\n    }\n}\n");

        let without = "<?php\nclass A\n{\n    protected readonly Logger $logger;\n}\n";
        let at = without.find("$logger").unwrap();
        let action = php_rewrites(without, at, &|_| false).into_iter().find(|action| action.title.starts_with("Move $logger")).unwrap();
        assert_eq!(apply(without, &action), "<?php\nclass A\n{\n    public function __construct(protected readonly Logger $logger)\n    {\n    }\n}\n");
    }

    #[test]
    fn property_with_default_attribute_or_assignment_stays() {
        for text in [
            "<?php\nclass A\n{\n    private Logger $l = null;\n}\n",
            "<?php\nclass A\n{\n    #[Required]\n    private Logger $l;\n}\n",
            "<?php\nclass A\n{\n    private string $l;\n}\n",
            "<?php\nclass A\n{\n    private Logger $l;\n    public function __construct() { $this->l = new Logger(); }\n}\n",
        ] {
            let at = text.find("$l").unwrap();
            assert!(php_rewrites(text, at, &|_| false).iter().all(|action| !action.title.starts_with("Move $l")), "{text}");
        }
    }
}
