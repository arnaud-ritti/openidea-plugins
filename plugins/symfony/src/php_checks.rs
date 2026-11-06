//! Checks of PHP classes: console command mistakes and deprecated Symfony
//! APIs. Written from the Symfony upgrade notes and component changelogs.

use std::ops::Range;

use crate::checks::{Context, Finding};
use crate::index::Index;
use crate::php_text::{attributes, class_model, imports, mask, matching_close, read_word, skip_whitespace, ClassModel, Target};

const AS_COMMAND: &str = "Symfony\\Component\\Console\\Attribute\\AsCommand";
const ATTRIBUTE_NAMESPACE: &str = "Symfony\\Component\\DependencyInjection\\Attribute\\";
const TAGGED_CLASSES: [&str; 4] = ["TaggedIterator", "TaggedLocator", "AutowireIterator", "AutowireLocator"];
const DEFAULT_METHOD_ARGUMENTS: [&str; 2] = ["defaultIndexMethod", "defaultPriorityMethod"];
const FROM_CALLABLE_CONFLICTS: [&str; 8] = ["alias", "parent", "synthetic", "file", "arguments", "properties", "configurator", "calls"];

const ABSTRACT_CONTROLLER: &str = "Symfony\\Bundle\\FrameworkBundle\\Controller\\AbstractController";
const LEGACY_CONTROLLER: &str = "Symfony\\Bundle\\FrameworkBundle\\Controller\\Controller";
const ABSTRACT_FORM_TYPE: &str = "Symfony\\Component\\Form\\AbstractType";
const ABSTRACT_TWIG_EXTENSION: &str = "Twig\\Extension\\AbstractExtension";
const CONSOLE_COMMAND: &str = "Symfony\\Component\\Console\\Command\\Command";
const SERVICE_SUBSCRIBER: &str = "Symfony\\Contracts\\Service\\ServiceSubscriberInterface";
const CONTAINER_INTERFACES: [&str; 2] = ["Psr\\Container\\ContainerInterface", "Symfony\\Component\\DependencyInjection\\ContainerInterface"];
const SUPERGLOBALS: [&str; 7] = ["$_GET", "$_POST", "$_COOKIE", "$_FILES", "$_REQUEST", "$_SESSION", "$_SERVER"];
const DEBUG_CALLS: [&str; 3] = ["var_dump(", "dd(", "die("];
const HTTP_KERNEL_EVENTS: &str = "Symfony\\Component\\HttpKernel\\Event\\";
const RENAMED_KERNEL_EVENTS: [(&str, &str); 7] = [
    ("GetResponseEvent", "RequestEvent"),
    ("FilterResponseEvent", "ResponseEvent"),
    ("GetResponseForExceptionEvent", "ExceptionEvent"),
    ("PostResponseEvent", "TerminateEvent"),
    ("GetResponseForControllerResultEvent", "ViewEvent"),
    ("FilterControllerEvent", "ControllerEvent"),
    ("FilterControllerArgumentsEvent", "ControllerArgumentsEvent"),
];
/// Form options removed in Symfony 3.0 (UPGRADE-3.0.md, Form) with their replacements.
const REMOVED_FORM_OPTIONS: [(&str, &str); 8] = [
    ("choices_as_values", "nothing: choices are always values"),
    ("empty_value", "placeholder"),
    ("read_only", "attr['readonly']"),
    ("intention", "csrf_token_id"),
    ("cascade_validation", "a Valid constraint"),
    ("max_length", "attr['maxlength']"),
    ("virtual", "inherit_data"),
    ("precision", "scale"),
];

/// UPGRADE-4.3.md, HttpKernel: the `GetResponseEvent` family was renamed.
const KERNEL_EVENT_RENAME_SINCE: (u32, u32) = (4, 3);
/// UPGRADE-5.4.md, FrameworkBundle: "Deprecate `AbstractController::getDoctrine()`".
const GET_DOCTRINE_SINCE: (u32, u32) = (5, 4);
/// UPGRADE-6.1.md, Console: `Command::$defaultName` and `$defaultDescription` give way to `#[AsCommand]`.
const DEFAULT_NAME_SINCE: (u32, u32) = (6, 1);
/// UPGRADE-6.2.md: `Security\Core\Security` moved to SecurityBundle, `Request::getContentType()` renamed.
const SECURITY_CLASS_SINCE: (u32, u32) = (6, 2);
const CONTENT_TYPE_SINCE: (u32, u32) = (6, 2);

/// UPGRADE-8.1.md, Filesystem: "Deprecate calling `Filesystem::mirror()` with option `copy_on_windows`".
const MIRROR_COPY_ON_WINDOWS_SINCE: (u32, u32) = (8, 1);
/// UPGRADE-8.1.md, DependencyInjection: "Deprecate default index/priority methods when defining tagged locators/iterators".
const TAGGED_DEFAULT_METHOD_SINCE: (u32, u32) = (8, 1);
/// UPGRADE-8.1.md, DependencyInjection: "Deprecate configuring options `alias`, `parent`, ... when using `from_callable`".
const FROM_CALLABLE_OPTION_SINCE: (u32, u32) = (8, 1);

fn reached(index: &Index, gate: (u32, u32)) -> bool {
    index.framework_version.is_some_and(|version| version >= gate)
}

/// Offsets of the top-level arguments of the call whose `(` is at `open`.
pub(crate) fn arguments(masked: &str, open: usize) -> Vec<Range<usize>> {
    let Some(close) = matching_close(masked, open) else { return Vec::new() };
    let mut found = Vec::new();
    let mut depth = 0usize;
    let mut start = open + 1;

    for (offset, byte) in masked[open + 1..close].bytes().enumerate() {
        let at = open + 1 + offset;
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                found.push(start..at);
                start = at + 1;
            }
            _ => {}
        }
    }

    if !masked[start..close].trim().is_empty() {
        found.push(start..close);
    }

    found
}

pub(crate) fn trimmed(text: &str, range: Range<usize>) -> Range<usize> {
    let slice = &text[range.clone()];
    let start = range.start + (slice.len() - slice.trim_start().len());
    start..start + slice.trim().len()
}

/// The argument written as `name: value`: the ranges of the name and the value.
pub(crate) fn named(masked: &str, argument: Range<usize>) -> Option<(Range<usize>, Range<usize>)> {
    let range = trimmed(masked, argument);
    let name = read_word(masked, range.start);
    let after = skip_whitespace(masked, name.end);
    let is_named = !name.is_empty() && masked[after..range.end].starts_with(':') && !masked[after..range.end].starts_with("::");
    if !is_named {
        return None;
    }

    let value = trimmed(masked, after + 1..range.end);
    Some((name, value))
}

/// String keys (`'name' =>`) inside `range` with their depth below it.
fn string_keys(masked: &str, text: &str, range: Range<usize>) -> Vec<(String, Range<usize>, usize)> {
    let bytes = masked.as_bytes();
    let mut found = Vec::new();
    let mut depth = 0usize;
    let mut at = range.start;

    while at < range.end {
        match bytes[at] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            quote @ (b'\'' | b'"') => {
                let Some(length) = masked[at + 1..range.end].bytes().position(|byte| byte == quote) else { break };
                let close = at + 1 + length;
                if masked[close + 1..].trim_start().starts_with("=>") {
                    found.push((text[at + 1..close].to_owned(), at + 1..close, depth));
                }
                at = close;
            }
            _ => {}
        }
        at += 1;
    }

    found
}

pub fn commands(text: &str) -> Vec<Finding> {
    let masked = mask(text);
    let mut found = invokable_return_types(text, &masked);
    found.extend(input_modes(&masked));
    found.sort_by_key(|finding| finding.start);
    found
}

fn invokable_return_types(text: &str, masked: &str) -> Vec<Finding> {
    let mut found = Vec::new();

    for attribute in attributes(text) {
        let is_command = attribute.name == AS_COMMAND && matches!(attribute.target, Target::Class { .. });
        if !is_command {
            continue;
        }

        let Some(open) = masked[attribute.range.end..].find('{').map(|at| attribute.range.end + at) else { continue };
        let Some(close) = matching_close(masked, open) else { continue };
        let Some(method) = invoke_in(masked, open + 1..close) else { continue };

        let name = method.start..method.end;
        let after_parameters = skip_whitespace(masked, method.end);
        if !masked[after_parameters..].starts_with('(') {
            continue;
        }
        let Some(parameters_end) = matching_close(masked, after_parameters) else { continue };

        let after = skip_whitespace(masked, parameters_end + 1);
        let return_type = match masked[after..].starts_with(':') {
            true => {
                let end = masked[after..].find(['{', ';']).map_or(masked.len(), |at| after + at);
                masked[after + 1..end].trim()
            }
            false => "",
        };

        let words = return_type.split(|c: char| !c.is_alphanumeric()).filter(|word| !word.is_empty());
        let mut words = words.map(str::to_ascii_lowercase);
        if words.any(|word| word == "int" || word == "never") {
            continue;
        }

        found.push(Finding::hint(name, "command-invokable-return-type", "The __invoke() method of a command should return an int, Command::SUCCESS or Command::FAILURE.".to_owned()));
    }

    found
}

/// Name of `function __invoke` declared directly in the class body.
fn invoke_in(masked: &str, body: Range<usize>) -> Option<Range<usize>> {
    let mut depth = 0usize;
    let mut at = body.start;

    while at < body.end {
        match masked.as_bytes()[at] {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'f' if depth == 0 && masked[at..body.end].starts_with("function") => {
                let name = read_word(masked, skip_whitespace(masked, at + "function".len()));
                if masked[name.clone()].eq_ignore_ascii_case("__invoke") {
                    return Some(name);
                }
            }
            _ => {}
        }
        at += 1;
    }

    None
}

struct ModeCall {
    callee: &'static str,
    owner: &'static str,
    position: usize,
    base_modes: &'static [&'static str],
}

const ARGUMENT_MODES: &[&str] = &["REQUIRED", "OPTIONAL"];
const OPTION_MODES: &[&str] = &["VALUE_NONE", "VALUE_REQUIRED", "VALUE_OPTIONAL"];
const MODE_CALLS: [ModeCall; 4] = [
    ModeCall { callee: "new InputArgument", owner: "InputArgument", position: 1, base_modes: ARGUMENT_MODES },
    ModeCall { callee: "->addArgument", owner: "InputArgument", position: 1, base_modes: ARGUMENT_MODES },
    ModeCall { callee: "new InputOption", owner: "InputOption", position: 2, base_modes: OPTION_MODES },
    ModeCall { callee: "->addOption", owner: "InputOption", position: 2, base_modes: OPTION_MODES },
];

fn input_modes(masked: &str) -> Vec<Finding> {
    let mut found = Vec::new();

    for call in &MODE_CALLS {
        for (at, _) in masked.match_indices(call.callee) {
            let starts_word = call.callee.starts_with("->") || masked[..at].chars().next_back().is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '_' | '$' | '\\')));
            let open = skip_whitespace(masked, at + call.callee.len());
            if !starts_word || !masked[open..].starts_with('(') {
                continue;
            }

            let mode = arguments(masked, open).into_iter().enumerate().find_map(|(position, argument)| match named(masked, argument.clone()) {
                Some((name, value)) => (&masked[name] == "mode").then_some(value),
                None => (position == call.position).then(|| trimmed(masked, argument)),
            });
            let Some(mode) = mode else { continue };

            if base_mode_count(&masked[mode.clone()], call) >= 2 {
                let message = format!("Only one of {} can be set; Symfony 8.1 deprecates combining them.", call.base_modes.iter().map(|mode| format!("{}::{mode}", call.owner)).collect::<Vec<_>>().join(", "));
                found.push(Finding::warn(mode, "console-input-mode", message));
            }
        }
    }

    found
}

fn base_mode_count(expression: &str, call: &ModeCall) -> usize {
    let mut modes: Vec<&str> = expression
        .split('|')
        .filter_map(|term| term.trim().strip_prefix('\\').unwrap_or(term.trim()).rsplit_once("::"))
        .filter(|(owner, constant)| owner.rsplit('\\').next() == Some(call.owner) && call.base_modes.contains(constant))
        .map(|(_, constant)| constant)
        .collect();
    modes.sort_unstable();
    modes.dedup();
    modes.len()
}

pub fn deprecations(ctx: &Context, text: &str) -> Vec<Finding> {
    let masked = mask(text);
    let mut found = Vec::new();

    if reached(ctx.index, MIRROR_COPY_ON_WINDOWS_SINCE) {
        found.extend(mirror_copy_on_windows(&masked, text));
    }

    if reached(ctx.index, TAGGED_DEFAULT_METHOD_SINCE) {
        found.extend(tagged_default_methods(&masked, text));
    }

    if reached(ctx.index, FROM_CALLABLE_OPTION_SINCE) {
        found.extend(from_callable_conflicts(&masked, text));
    }

    let model = class_model(text);
    let mut add = |gate: (u32, u32), check: &dyn Fn() -> Vec<Finding>| {
        if reached(ctx.index, gate) {
            found.extend(check());
        }
    };
    add(KERNEL_EVENT_RENAME_SINCE, &|| renamed_kernel_events(&masked));
    add(GET_DOCTRINE_SINCE, &|| controller_get_doctrine(&masked, model.as_ref()));
    add(DEFAULT_NAME_SINCE, &|| command_default_name(&masked, model.as_ref()));
    add(SECURITY_CLASS_SINCE, &|| moved_security_class(&masked));
    add(CONTENT_TYPE_SINCE, &|| request_get_content_type(&masked, text));

    found.extend(crate::practice_checks::deprecations(&masked, text, model.as_ref(), &|gate| reached(ctx.index, gate)));
    found.sort_by_key(|finding| finding.start);
    found
}

/// Offsets in `range` where `needle` starts a token: not the tail of a longer name or member access.
pub(crate) fn occurrences<'a>(masked: &'a str, range: Range<usize>, needle: &'a str) -> impl Iterator<Item = usize> + 'a {
    masked[range.clone()].match_indices(needle).filter_map(move |(at, _)| {
        let at = range.start + at;
        let before = masked[..at].bytes().last();
        let glued = before.is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'\\' | b'$' | b'>' | b':'));
        (!glued).then_some(at)
    })
}

fn is_controller(model: &ClassModel) -> bool {
    matches!(model.parent.as_deref(), Some(ABSTRACT_CONTROLLER | LEGACY_CONTROLLER))
}

fn renamed_kernel_events(masked: &str) -> Vec<Finding> {
    let mut found = Vec::new();

    for (old, new) in RENAMED_KERNEL_EVENTS {
        let needle = format!("{HTTP_KERNEL_EVENTS}{old}");
        for (at, _) in masked.match_indices(&needle) {
            let end = at + needle.len();
            if masked.as_bytes().get(end).is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'\\')) {
                continue;
            }

            let message = format!("{old} was deprecated in Symfony 4.3 and removed in 5.0: use {new}.");
            found.push(Finding::warn(at + HTTP_KERNEL_EVENTS.len()..end, "http-kernel-event-argument", message));
        }
    }

    found
}

fn controller_get_doctrine(masked: &str, model: Option<&ClassModel>) -> Vec<Finding> {
    let Some(model) = model.filter(|model| is_controller(model) && model.method("getDoctrine").is_none()) else { return Vec::new() };

    occurrences(masked, model.body.clone(), "$this->getDoctrine(")
        .map(|at| {
            let name = at + "$this->".len();
            Finding::warn(name..name + "getDoctrine".len(), "controller-get-doctrine", "AbstractController::getDoctrine() is deprecated since Symfony 5.4 and removed in 6.0: inject ManagerRegistry or EntityManagerInterface.".to_owned())
        })
        .collect()
}

fn command_default_name(masked: &str, model: Option<&ClassModel>) -> Vec<Finding> {
    let Some(model) = model.filter(|model| model.parent.as_deref() == Some(CONSOLE_COMMAND)) else { return Vec::new() };
    let mut found = Vec::new();

    for property in ["$defaultName", "$defaultDescription"] {
        for at in occurrences(masked, model.body.clone(), property) {
            let declared = masked[..at].trim_end().ends_with("static");
            let end = at + property.len();
            if declared && !masked.as_bytes().get(end).is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_') {
                let message = format!("Command::{property} is deprecated since Symfony 6.1: use the #[AsCommand] attribute.");
                found.push(Finding::warn(at..end, "command-default-name", message));
            }
        }
    }

    found
}

fn moved_security_class(masked: &str) -> Vec<Finding> {
    let old = "Symfony\\Component\\Security\\Core\\Security";

    masked
        .match_indices(old)
        .filter(|(at, _)| !masked.as_bytes().get(at + old.len()).is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'\\')))
        .map(|(at, _)| {
            let message = "Core\\Security was deprecated in Symfony 6.2 and removed in 7.0: use Symfony\\Bundle\\SecurityBundle\\Security.".to_owned();
            Finding::warn(at..at + old.len(), "security-class-moved", message)
        })
        .collect()
}

/// Only the variable `$request` in a file that imports `HttpFoundation\Request`
/// and types a parameter `Request $request`: the receiver's type is otherwise unknown.
fn request_get_content_type(masked: &str, text: &str) -> Vec<Finding> {
    let typed = text.contains("Symfony\\Component\\HttpFoundation\\Request;") && masked.contains("Request $request");
    if !typed {
        return Vec::new();
    }

    occurrences(masked, 0..masked.len(), "$request->getContentType(")
        .map(|at| {
            let name = at + "$request->".len();
            Finding::warn(name..name + "getContentType".len(), "request-get-content-type", "Request::getContentType() is deprecated since Symfony 6.2: use getContentTypeFormat().".to_owned())
        })
        .collect()
}

/// Style and removed-API advice for controllers, forms, Twig extensions and
/// constructors; the class's role comes from its resolved parent class.
pub fn practices(text: &str) -> Vec<Finding> {
    let masked = mask(text);
    let mut found = legacy_twig_classes(&masked);
    let Some(model) = class_model(text) else { return found };

    found.extend(controller_practices(&masked, &model));
    found.extend(constructor_container(&model));
    found.extend(twig_extension_name(&model));
    found.extend(form_type_removals(text, &masked, &model));
    found.extend(crate::practice_checks::practices(text, &masked, &model));
    found.sort_by_key(|finding| finding.start);
    found
}

fn controller_practices(masked: &str, model: &ClassModel) -> Vec<Finding> {
    let mut found = Vec::new();
    if !is_controller(model) {
        return found;
    }

    if let Some(range) = model.parent_range.clone().filter(|_| model.parent.as_deref() == Some(LEGACY_CONTROLLER)) {
        found.push(Finding::hint(range, "controller-base-class", "The Controller base class was removed in Symfony 5.0: extend AbstractController and inject services.".to_owned()));
    }

    if model.method("get").is_none() {
        for at in occurrences(masked, model.body.clone(), "$this->get(").chain(occurrences(masked, model.body.clone(), "$this->container->get(")) {
            let name = at + masked[at..].find("get(").unwrap_or(0);
            found.push(Finding::hint(name..name + 3, "controller-container-get", "Fetching a service from the container hides the controller's dependencies: inject it as an action argument or constructor parameter.".to_owned()));
        }
    }

    for global in SUPERGLOBALS {
        for at in occurrences(masked, model.body.clone(), global) {
            let end = at + global.len();
            if masked.as_bytes().get(end).is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_') {
                continue;
            }

            found.push(Finding::hint(at..end, "controller-superglobal", format!("Read {global} through the Request object instead.")));
        }
    }

    for call in DEBUG_CALLS {
        for at in occurrences(masked, model.body.clone(), call) {
            let name = call.trim_end_matches('(');
            found.push(Finding::hint(at..at + name.len(), "controller-debug-call", format!("{name}() looks like leftover debugging in a controller.")));
        }
    }

    found
}

fn constructor_container(model: &ClassModel) -> Vec<Finding> {
    let implements_subscriber = model.interfaces.iter().any(|interface| interface == SERVICE_SUBSCRIBER);
    let Some(constructor) = model.method("__construct").filter(|_| !implements_subscriber) else { return Vec::new() };

    constructor
        .params
        .iter()
        .filter(|param| !param.has_attribute && param.type_name.as_deref().is_some_and(|name| CONTAINER_INTERFACES.contains(&name)))
        .map(|param| Finding::hint(param.range.clone(), "constructor-container-injection", "Injecting the container makes the dependencies of this class invisible: inject the services it uses, or a service locator.".to_owned()))
        .collect()
}

fn twig_extension_name(model: &ClassModel) -> Vec<Finding> {
    let method = model.method("getName").filter(|_| model.parent.as_deref() == Some(ABSTRACT_TWIG_EXTENSION));

    method.map(|method| Finding::hint(method.name_range.clone(), "twig-extension-get-name", "Twig extensions need no getName(); it was removed in Twig 2.".to_owned())).into_iter().collect()
}

fn legacy_twig_classes(masked: &str) -> Vec<Finding> {
    masked
        .match_indices("Twig_")
        .filter(|(at, _)| {
            let glued = masked[..*at].bytes().last().is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'>' | b':'));
            let named = masked.as_bytes().get(at + 5).is_some_and(u8::is_ascii_uppercase);
            !glued && named
        })
        .map(|(at, _)| {
            let end = at + masked[at..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(masked.len() - at);
            Finding::warn(at..end, "twig-legacy-class", format!("{} is a legacy class name removed in Twig 3: use the namespaced Twig\\... class.", &masked[at..end]))
        })
        .collect()
}

fn form_type_removals(text: &str, masked: &str, model: &ClassModel) -> Vec<Finding> {
    let mut found = Vec::new();
    if model.parent.as_deref() != Some(ABSTRACT_FORM_TYPE) {
        return found;
    }

    for (old, new) in [("setDefaultOptions", "configureOptions(OptionsResolver $resolver)"), ("getName", "getBlockPrefix()")] {
        if let Some(method) = model.method(old) {
            found.push(Finding::warn(method.name_range.clone(), "form-type-removed-method", format!("{old}() of form types was removed in Symfony 3.0: use {new}.")));
        }
    }

    for (key, range, _) in string_keys(masked, text, model.body.clone()) {
        if let Some((_, replacement)) = REMOVED_FORM_OPTIONS.iter().find(|(old, _)| *old == key) {
            found.push(Finding::warn(range, "form-removed-option", format!("The \"{key}\" form option was removed in Symfony 3.0: use {replacement}.")));
        }
    }

    found
}

fn mirror_copy_on_windows(masked: &str, text: &str) -> Vec<Finding> {
    let mut found = Vec::new();
    if !text.contains("Symfony\\Component\\Filesystem") {
        return found;
    }

    for (at, _) in masked.match_indices("->mirror") {
        let open = skip_whitespace(masked, at + "->mirror".len());
        let Some(close) = masked[open..].starts_with('(').then(|| matching_close(masked, open)).flatten() else { continue };

        for (key, range, _) in string_keys(masked, text, open..close) {
            if key == "copy_on_windows" {
                let message = "The \"copy_on_windows\" option of Filesystem::mirror() is deprecated since Symfony 8.1: use \"follow_symlinks\" instead.".to_owned();
                found.push(Finding::warn(range, "filesystem-mirror-copy-on-windows", message));
            }
        }
    }

    found
}

fn tagged_default_methods(masked: &str, text: &str) -> Vec<Finding> {
    let mut found = Vec::new();
    let mut calls: Vec<Range<usize>> = Vec::new();

    for attribute in attributes(text) {
        let Some(class) = attribute.name.strip_prefix(ATTRIBUTE_NAMESPACE) else { continue };
        if TAGGED_CLASSES.contains(&class) {
            calls.push(attribute.arguments.start - 1..attribute.arguments.end + 1);
        }
    }

    let imports = imports(text);
    for class in TAGGED_CLASSES {
        let fqcn = format!("{ATTRIBUTE_NAMESPACE}{class}");
        let alias = imports.iter().find(|import| import.fqcn == fqcn).map(|import| import.alias.as_str());
        for name in [alias.map(str::to_owned), Some(format!("\\{fqcn}"))].into_iter().flatten() {
            for (at, _) in masked.match_indices(&format!("new {name}")) {
                let open = skip_whitespace(masked, at + "new ".len() + name.len());
                if masked[open..].starts_with('(') && matching_close(masked, open).is_some() {
                    calls.push(open..matching_close(masked, open).unwrap_or(open) + 1);
                }
            }
        }
    }

    for call in calls {
        for argument in arguments(masked, call.start) {
            let Some((name, value)) = named(masked, argument) else { continue };
            let is_literal = matches!(text.as_bytes().get(value.start), Some(b'\'' | b'"')) && value.len() > 2;
            if DEFAULT_METHOD_ARGUMENTS.contains(&&masked[name.clone()]) && is_literal {
                let message = format!("\"{}\" is deprecated since Symfony 8.1: use the #[AsTaggedItem] attribute instead.", &masked[name.clone()]);
                found.push(Finding::warn(name, "tagged-collection-default-method", message));
            }
        }
    }

    found
}

fn from_callable_conflicts(masked: &str, text: &str) -> Vec<Finding> {
    let mut found = Vec::new();

    for (key, range, _) in string_keys(masked, text, 0..masked.len()) {
        if key != "from_callable" {
            continue;
        }

        let Some(open) = enclosing_array(masked, range.start) else { continue };
        let Some(close) = matching_close(masked, open) else { continue };

        for (sibling, sibling_range, depth) in string_keys(masked, text, open + 1..close) {
            if depth == 0 && FROM_CALLABLE_CONFLICTS.contains(&sibling.as_str()) {
                let message = format!("Configuring \"{sibling}\" together with \"from_callable\" is deprecated since Symfony 8.1.");
                found.push(Finding::warn(sibling_range, "from-callable-option", message));
            }
        }
    }

    found
}

/// Offset of the `[` or `array(` that holds the key at `at`.
fn enclosing_array(masked: &str, at: usize) -> Option<usize> {
    let mut depth = 0usize;

    for (position, byte) in masked[..at].bytes().enumerate().rev() {
        match byte {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' if depth > 0 => depth -= 1,
            b'[' => return Some(position),
            b'(' => return masked[..position].trim_end().to_ascii_lowercase().ends_with("array").then_some(position),
            b'{' => return None,
            _ => {}
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::{Diagnostic, Event, Plugin, Position, Range as TextRange};

    use super::*;
    use crate::Main;

    const PATH: &str = "/project/src/Command/XCommand.php";

    fn index(version: Option<(u32, u32)>) -> Index {
        Index { framework_version: version, ..Index::default() }
    }

    fn command(method: &str) -> String {
        format!("<?php\nnamespace App\\Command;\n\nuse Symfony\\Component\\Console\\Attribute\\AsCommand;\n\n#[AsCommand(name: 'app:x')]\nclass XCommand\n{{\n    {method}\n}}\n")
    }

    fn texts(text: &str, found: &[Finding]) -> Vec<(&'static str, String)> {
        found.iter().map(|finding| (finding.code, text[finding.start..finding.end].to_owned())).collect()
    }

    fn mode_texts(code: &str) -> Vec<(&'static str, String)> {
        let text = format!("<?php\n{code}\n");
        texts(&text, &commands(&text))
    }

    fn deprecations_in(version: Option<(u32, u32)>, text: &str) -> Vec<(&'static str, String)> {
        let index = index(version);
        let read = |_: &str| None;
        let ctx = Context::new(&index, &read);
        texts(text, &super::deprecations(&ctx, text))
    }

    const MIRROR: &str = "<?php\nuse Symfony\\Component\\Filesystem\\Filesystem;\n\nfunction f(Filesystem $fs) {\n    $fs->mirror('a', 'b', null, ['copy_on_windows' => true]);\n}\n";
    const TAGGED: &str = "<?php\nuse Symfony\\Component\\DependencyInjection\\Attribute\\AutowireIterator;\nuse Symfony\\Component\\DependencyInjection\\Attribute\\TaggedLocator;\n\nclass A\n{\n    public function __construct(\n        #[AutowireIterator('app.h', defaultPriorityMethod: 'getPriority')] iterable $a,\n        #[TaggedLocator(tag: 'app.h', defaultIndexMethod: 'getKey')] $b,\n    ) {}\n}\n";
    const CALLABLE: &str = "<?php\nreturn [\n    'services' => [\n        'app.x' => [\n            'from_callable' => [A::class, 'make'],\n            'parent' => 'app.base',\n            'calls' => [['setUp', []]],\n        ],\n    ],\n];\n";

    #[test]
    fn invokable_command_without_int_return_type_is_a_hint() {
        for method in ["public function __invoke() {}", "public function __invoke(): void {}", "public function __invoke(): string {}"] {
            let text = command(method);
            let found = commands(&text);
            assert_eq!(texts(&text, &found), [("command-invokable-return-type", "__invoke".to_owned())], "{method}");
            assert_eq!(found[0].severity, openidea_plugin::Severity::Hint);
        }
    }

    #[test]
    fn invokable_command_with_int_or_union_int_is_quiet() {
        for method in ["public function __invoke(): int { return 0; }", "public function __invoke(): int|false {}", "public function __invoke(): ?int {}", "public function __invoke(): never {}"] {
            assert!(commands(&command(method)).is_empty(), "{method}");
        }
    }

    #[test]
    fn invoke_without_as_command_is_quiet() {
        let text = "<?php\nnamespace App;\n\nclass Handler\n{\n    public function __invoke() {}\n}\n";
        assert!(commands(text).is_empty());

        let text = command("public function execute() {}");
        assert!(commands(&text).is_empty());
    }

    #[test]
    fn unresolvable_as_command_name_is_quiet() {
        let text = "<?php\nnamespace App\\Command;\n\n#[AsCommand(name: 'app:x')]\nclass XCommand\n{\n    public function __invoke() {}\n}\n";
        assert!(commands(text).is_empty());

        let other = text.replace("#[AsCommand", "#[Other\\AsCommand");
        assert!(commands(&other).is_empty());
    }

    #[test]
    fn or_ed_base_modes_warn_for_arguments_and_options() {
        assert_eq!(
            mode_texts("$c->addArgument('n', InputArgument::REQUIRED | InputArgument::OPTIONAL);"),
            [("console-input-mode", "InputArgument::REQUIRED | InputArgument::OPTIONAL".to_owned())]
        );
        assert_eq!(
            mode_texts("$a = new InputArgument('n', InputArgument::REQUIRED|InputArgument::OPTIONAL|InputArgument::IS_ARRAY);"),
            [("console-input-mode", "InputArgument::REQUIRED|InputArgument::OPTIONAL|InputArgument::IS_ARRAY".to_owned())]
        );
        assert_eq!(
            mode_texts("$c->addOption('o', 'x', InputOption::VALUE_NONE | InputOption::VALUE_REQUIRED, 'help');"),
            [("console-input-mode", "InputOption::VALUE_NONE | InputOption::VALUE_REQUIRED".to_owned())]
        );
        assert_eq!(
            mode_texts("$o = new InputOption('o', mode: InputOption::VALUE_OPTIONAL | InputOption::VALUE_NONE);"),
            [("console-input-mode", "InputOption::VALUE_OPTIONAL | InputOption::VALUE_NONE".to_owned())]
        );
    }

    #[test]
    fn modifiers_and_numeric_modes_are_quiet() {
        for code in [
            "$c->addArgument('n', InputArgument::REQUIRED | InputArgument::IS_ARRAY);",
            "$c->addOption('o', null, InputOption::VALUE_REQUIRED | InputOption::VALUE_IS_ARRAY);",
            "$c->addOption('o', null, InputOption::VALUE_NONE | InputOption::VALUE_NEGATABLE);",
            "$c->addArgument('n', 3);",
            "$c->addOption('o', null, 5);",
            "$c->addArgument('n', InputArgument::REQUIRED);",
            "$c->addOption('o', null, InputOption::VALUE_OPTIONAL, 'a | b');",
        ] {
            assert!(mode_texts(code).is_empty(), "{code}");
        }
    }

    #[test]
    fn console_input_mode_is_quiet_for_variables_and_unknown_constants() {
        for code in [
            "$c->addArgument('n', $mode);",
            "$c->addArgument('n', $a | InputArgument::OPTIONAL);",
            "$c->addArgument('n', self::MODE | Other::REQUIRED);",
            "$c->addOption('o', null, MyMode::VALUE_NONE | MyMode::VALUE_REQUIRED);",
            "$c->addArgument('n', InputOption::REQUIRED | InputOption::OPTIONAL);",
            "$c->addOption('o', null, InputOption::VALUE_NONE | InputArgument::REQUIRED);",
            "// $c->addArgument('n', InputArgument::REQUIRED | InputArgument::OPTIONAL);",
            "$c->addArgument('n', 'InputArgument::REQUIRED | InputArgument::OPTIONAL');",
            "$c->addArgumentX('n', InputArgument::REQUIRED | InputArgument::OPTIONAL);",
        ] {
            assert!(mode_texts(code).is_empty(), "{code}");
        }
    }

    #[test]
    fn filesystem_mirror_copy_on_windows_warns_at_the_gate() {
        let found = deprecations_in(Some((8, 1)), MIRROR);
        assert_eq!(found, [("filesystem-mirror-copy-on-windows", "copy_on_windows".to_owned())]);
        assert_eq!(deprecations_in(Some((9, 0)), MIRROR).len(), 1);
    }

    #[test]
    fn filesystem_mirror_copy_on_windows_is_quiet_below_the_gate() {
        assert!(deprecations_in(Some((8, 0)), MIRROR).is_empty());
        assert!(deprecations_in(Some((7, 4)), MIRROR).is_empty());
        assert!(deprecations_in(None, MIRROR).is_empty());
    }

    #[test]
    fn filesystem_mirror_copy_on_windows_is_quiet_for_dynamic_values() {
        for call in [
            "$fs->mirror('a', 'b', null, $options);",
            "$fs->mirror('a', 'b', null, [$key => true]);",
            "$fs->mirror('a', 'b', null, ['follow_symlinks' => true]);",
            "$fs->mirror('a', 'b', null, ['copy_on_windows_x' => true]);",
            "$fs->mirror('a', 'b', null, ['x' => 'copy_on_windows']);",
            "$fs->copy('a', 'b', ['copy_on_windows' => true]);",
            "// $fs->mirror('a', 'b', null, ['copy_on_windows' => true]);",
        ] {
            let text = format!("<?php\nuse Symfony\\Component\\Filesystem\\Filesystem;\nfunction f($fs) {{\n    {call}\n}}\n");
            assert!(deprecations_in(Some((8, 1)), &text).is_empty(), "{call}");
        }

        let without_import = "<?php\nfunction f($fs) {\n    $fs->mirror('a', 'b', null, ['copy_on_windows' => true]);\n}\n";
        assert!(deprecations_in(Some((8, 1)), without_import).is_empty());
    }

    #[test]
    fn tagged_collection_default_method_warns_at_the_gate() {
        let found = deprecations_in(Some((8, 1)), TAGGED);
        assert_eq!(
            found,
            [
                ("tagged-collection-default-method", "defaultPriorityMethod".to_owned()),
                ("tagged-collection-default-method", "defaultIndexMethod".to_owned())
            ]
        );

        let new = "<?php\nuse Symfony\\Component\\DependencyInjection\\Attribute\\TaggedIterator;\n\n$a = new TaggedIterator('app.h', defaultIndexMethod: 'getKey');\n";
        assert_eq!(deprecations_in(Some((8, 1)), new), [("tagged-collection-default-method", "defaultIndexMethod".to_owned())]);
    }

    #[test]
    fn tagged_collection_default_method_is_quiet_below_the_gate() {
        assert!(deprecations_in(Some((8, 0)), TAGGED).is_empty());
        assert!(deprecations_in(None, TAGGED).is_empty());
    }

    #[test]
    fn tagged_collection_default_method_is_quiet_for_dynamic_values() {
        let imports = "use Symfony\\Component\\DependencyInjection\\Attribute\\TaggedIterator;\n";
        for (head, code) in [
            (imports, "$a = new TaggedIterator('app.h', defaultIndexMethod: $method);"),
            (imports, "$a = new TaggedIterator('app.h', defaultIndexMethod: null);"),
            (imports, "$a = new TaggedIterator('app.h', indexAttribute: 'key');"),
            (imports, "$a = new TaggedIterator('app.h', 'x', 'getKey');"),
            ("", "$a = new TaggedIterator('app.h', defaultIndexMethod: 'getKey');"),
            ("use App\\TaggedIterator;\n", "$a = new TaggedIterator('app.h', defaultIndexMethod: 'getKey');"),
            (imports, "$a = new Other('app.h', defaultIndexMethod: 'getKey');"),
            (imports, "// $a = new TaggedIterator('app.h', defaultIndexMethod: 'getKey');"),
        ] {
            let text = format!("<?php\n{head}{code}\n");
            assert!(deprecations_in(Some((8, 1)), &text).is_empty(), "{code}");
        }
    }

    #[test]
    fn from_callable_option_warns_at_the_gate() {
        let found = deprecations_in(Some((8, 1)), CALLABLE);
        assert_eq!(found, [("from-callable-option", "parent".to_owned()), ("from-callable-option", "calls".to_owned())]);
    }

    #[test]
    fn from_callable_option_is_quiet_below_the_gate() {
        assert!(deprecations_in(Some((8, 0)), CALLABLE).is_empty());
        assert!(deprecations_in(None, CALLABLE).is_empty());
    }

    #[test]
    fn from_callable_option_is_quiet_for_dynamic_values() {
        for body in [
            "'from_callable' => [A::class, 'make'], 'public' => true, 'class' => 'X',",
            "'from_callable' => [A::class, 'make'], $key => 'x',",
            "'class' => 'X', 'parent' => 'p',",
            "'from_callable' => [A::class, 'make'], 'tags' => ['parent' => 'x'],",
            "'from_callable' => [A::class, 'make'], 'x' => 'parent',",
        ] {
            let text = format!("<?php\nreturn ['services' => ['a' => [{body}], 'b' => ['parent' => 'p']]];\n");
            assert!(deprecations_in(Some((8, 1)), &text).is_empty(), "{body}");
        }
    }

    #[test]
    fn deprecations_ranges_use_utf16_columns_with_multibyte_text() {
        let text = MIRROR.replace("    $fs->mirror", "    /* \u{e9}\u{1F600} */ $fs->mirror");
        let found = run(Some((8, 1)), PATH, &text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].range.start.line, 4);
        assert_eq!(found[0].range.start.character, 4 + 10 + "$fs->mirror('a', 'b', null, ['".len() as u32);
    }

    #[test]
    fn deprecations_crlf_files_report_correct_lines() {
        let found = run(Some((8, 1)), PATH, &MIRROR.replace('\n', "\r\n"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].range.start.line, 4);
        assert_eq!(found[0].range.start.character, 4 + "$fs->mirror('a', 'b', null, ['".len() as u32);
    }

    #[test]
    fn deprecations_fixed_file_publishes_empty_list() {
        let host = MockHost::install();
        let mut plugin = project(&host);

        host.add_file("src/Command/XCommand.php", MIRROR);
        assert_eq!(published(&host, &mut plugin, Event::FileOpened(PATH.into())), ["filesystem-mirror-copy-on-windows"]);

        host.add_file("src/Command/XCommand.php", &MIRROR.replace(", ['copy_on_windows' => true]", ""));
        assert!(published(&host, &mut plugin, Event::FileSaved(PATH.into())).is_empty());
    }

    #[test]
    fn console_input_mode_counts_distinct_base_modes() {
        for code in [
            "$c->addArgument('n', InputArgument::REQUIRED | InputArgument::REQUIRED);",
            "$c->addOption('o', null, InputOption::VALUE_NONE | InputOption::VALUE_NONE);",
        ] {
            assert!(mode_texts(code).is_empty(), "{code}");
        }
    }

    #[test]
    fn php_group_ranges_use_utf16_columns_with_multibyte_text() {
        let text = "<?php /* é😀 */ $c->addArgument('n', InputArgument::REQUIRED | InputArgument::OPTIONAL);\n";
        let found = run(Some((8, 1)), PATH, text);
        assert_eq!(found[0].range.start, Position::new(0, 16 + 21));
    }

    #[test]
    fn php_group_crlf_files_report_correct_lines() {
        let text = command("public function __invoke() {}").replace('\n', "\r\n");
        let found = run(Some((8, 1)), PATH, &text);
        assert_eq!(found[0].range, TextRange::new(Position::new(8, 20), Position::new(8, 28)));
    }

    fn run(version: Option<(u32, u32)>, path: &str, text: &str) -> Vec<Diagnostic> {
        let read = |_: &str| None;
        crate::extra_problems(&index(version), crate::Groups::all_on(), &read, path, "php", text)
    }

    fn published(host: &MockHost, plugin: &mut Main, event: Event) -> Vec<String> {
        plugin.handle_events(&[event], &host.invocation());
        host.diagnostics().get(PATH).map(|found| found.iter().filter_map(|d| d.code.clone()).collect()).unwrap_or_default()
    }

    fn project(host: &MockHost) -> Main {
        host.add_file("composer.json", r#"{"require": {"symfony/framework-bundle": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#);
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();
        plugin
    }

    #[test]
    fn php_group_fixed_file_publishes_empty_list() {
        let host = MockHost::install();
        let mut plugin = project(&host);

        host.add_file("src/Command/XCommand.php", &command("public function __invoke() {}"));
        assert_eq!(published(&host, &mut plugin, Event::FileOpened(PATH.into())), ["command-invokable-return-type"]);

        host.add_file("src/Command/XCommand.php", &command("public function __invoke(): int { return 0; }"));
        assert!(published(&host, &mut plugin, Event::FileSaved(PATH.into())).is_empty());
    }

    #[test]
    fn php_group_missing_file_is_treated_as_absent() {
        let host = MockHost::install();
        let mut plugin = project(&host);
        assert!(published(&host, &mut plugin, Event::FileOpened("/project/src/Gone.php".into())).is_empty());
    }

    #[test]
    fn command_and_deprecation_groups_switch_off_independently() {
        let host = MockHost::install();
        let mut plugin = project(&host);
        let both = format!("{}{MIRROR}", command("public function __invoke() {}")).replace("<?php\n", "").replacen("namespace", "<?php\nnamespace", 1);
        host.add_file("src/Command/XCommand.php", &both);
        let open = || Event::FileOpened(PATH.into());

        assert_eq!(published(&host, &mut plugin, open()), ["command-invokable-return-type", "filesystem-mirror-copy-on-windows"]);

        host.set_setting("commandChecks", "false");
        assert_eq!(published(&host, &mut plugin, open()), ["filesystem-mirror-copy-on-windows"]);

        host.set_setting("commandChecks", "true");
        host.set_setting("deprecationChecks", "false");
        assert_eq!(published(&host, &mut plugin, open()), ["command-invokable-return-type"]);
    }

    fn practice_texts(text: &str) -> Vec<(&'static str, String)> {
        texts(text, &practices(text))
    }

    const CONTROLLER: &str = "<?php\nnamespace App\\Controller;\n\nuse Symfony\\Bundle\\FrameworkBundle\\Controller\\AbstractController;\n\nclass HomeController extends AbstractController\n{\n    public function index()\n    {\n        $mailer = $this->get('mailer');\n        $id = $_GET['id'];\n        var_dump($id);\n        $x = $this->container->get('x');\n    }\n}\n";

    #[test]
    fn controller_practices_hint_at_container_superglobals_and_debug_calls() {
        assert_eq!(
            practice_texts(CONTROLLER),
            [
                ("controller-container-get", "get".to_owned()),
                ("controller-superglobal", "$_GET".to_owned()),
                ("controller-debug-call", "var_dump".to_owned()),
                ("controller-container-get", "get".to_owned()),
            ]
        );
        assert!(practices(CONTROLLER).iter().all(|finding| finding.severity == openidea_plugin::Severity::Hint));
    }

    #[test]
    fn controller_practices_stay_quiet_outside_controllers_and_for_own_methods() {
        let other = CONTROLLER.replace("extends AbstractController", "extends BaseController");
        assert!(practice_texts(&other).is_empty());

        let foreign = CONTROLLER.replace("Symfony\\Bundle\\FrameworkBundle\\Controller\\AbstractController", "Acme\\AbstractController");
        assert!(practice_texts(&foreign).is_empty());

        let own = CONTROLLER.replace("public function index()", "public function get($id) { return $this->get($id); }\n    public function index()");
        assert!(practice_texts(&own).iter().all(|(code, _)| *code != "controller-container-get"));

        let member = "<?php\nuse Symfony\\Bundle\\FrameworkBundle\\Controller\\AbstractController;\nclass A extends AbstractController\n{\n    public function i() { $this->add(1); $other->get('x'); $this->adds(); dd_x(); $_GETTER; }\n}\n";
        assert!(practice_texts(member).is_empty());
    }

    #[test]
    fn legacy_controller_base_class_is_a_hint_on_the_parent_name() {
        let text = "<?php\nuse Symfony\\Bundle\\FrameworkBundle\\Controller\\Controller;\n\nclass A extends Controller\n{\n}\n";
        assert_eq!(practice_texts(text), [("controller-base-class", "Controller".to_owned())]);
    }

    #[test]
    fn container_injected_into_a_constructor_is_a_hint_unless_attributed_or_a_subscriber() {
        let text = "<?php\nnamespace App;\n\nuse Psr\\Container\\ContainerInterface;\n\nclass A\n{\n    public function __construct(private ContainerInterface $container, private int $n = 1)\n    {\n    }\n}\n";
        assert_eq!(practice_texts(text), [("constructor-container-injection", "private ContainerInterface $container".to_owned())]);

        let attributed = text.replace("private ContainerInterface", "#[AutowireLocator([])] private ContainerInterface");
        assert!(practice_texts(&attributed).is_empty());

        let subscriber = text.replace("class A", "class A implements ServiceSubscriberInterface").replace("use Psr", "use Symfony\\Contracts\\Service\\ServiceSubscriberInterface;\nuse Psr");
        assert!(practice_texts(&subscriber).is_empty());

        let other_type = text.replace("use Psr\\Container\\ContainerInterface;", "use Acme\\ContainerInterface;");
        assert!(practice_texts(&other_type).is_empty());
    }

    #[test]
    fn twig_extension_get_name_and_legacy_class_names_are_reported() {
        let text = "<?php\nnamespace App\\Twig;\n\nuse Twig\\Extension\\AbstractExtension;\n\nclass E extends AbstractExtension\n{\n    public function getName() { return 'e'; }\n}\n";
        assert_eq!(practice_texts(text), [("twig-extension-get-name", "getName".to_owned())]);

        let legacy = "<?php\nclass E extends \\Twig_Extension\n{\n    // Twig_Ignored\n    public function f() { $x = 'Twig_String'; $y = $this->Twig_Foo; new Twig_Environment(); }\n}\n";
        assert_eq!(practice_texts(legacy), [("twig-legacy-class", "Twig_Extension".to_owned()), ("twig-legacy-class", "Twig_Environment".to_owned())]);
    }

    #[test]
    fn form_type_removed_methods_and_options_are_reported() {
        let text = "<?php\nnamespace App\\Form;\n\nuse Symfony\\Component\\Form\\AbstractType;\n\nclass T extends AbstractType\n{\n    public function buildForm($builder, array $options)\n    {\n        $builder->add('a', null, ['empty_value' => '', 'label' => 'x', 'read_only' => true]);\n    }\n\n    public function setDefaultOptions($resolver) {}\n\n    public function getName() { return 't'; }\n}\n";
        assert_eq!(
            practice_texts(text),
            [("form-removed-option", "empty_value".to_owned()), ("form-removed-option", "read_only".to_owned()), ("form-type-removed-method", "setDefaultOptions".to_owned()), ("form-type-removed-method", "getName".to_owned())]
        );

        let plain = text.replace("extends AbstractType", "extends Other");
        assert!(practice_texts(&plain).is_empty());
    }

    #[test]
    fn deprecated_get_doctrine_and_default_name_need_their_version() {
        let controller = "<?php\nuse Symfony\\Bundle\\FrameworkBundle\\Controller\\AbstractController;\nclass A extends AbstractController\n{\n    public function i() { return $this->getDoctrine()->getManager(); }\n}\n";
        assert_eq!(deprecations_in(Some((5, 4)), controller), [("controller-get-doctrine", "getDoctrine".to_owned())]);
        assert!(deprecations_in(Some((5, 3)), controller).is_empty());
        assert!(deprecations_in(None, controller).is_empty());

        let command = "<?php\nuse Symfony\\Component\\Console\\Command\\Command;\nclass C extends Command\n{\n    protected static $defaultName = 'app:c';\n    protected static $defaultDescription = 'd';\n    public $defaultNameless;\n}\n";
        assert_eq!(deprecations_in(Some((6, 1)), command), [("command-default-name", "$defaultName".to_owned()), ("command-default-name", "$defaultDescription".to_owned())]);
        assert!(deprecations_in(Some((6, 0)), command).is_empty());
    }

    #[test]
    fn renamed_kernel_events_security_and_content_type_are_reported() {
        let events = "<?php\nuse Symfony\\Component\\HttpKernel\\Event\\GetResponseEvent;\nuse Symfony\\Component\\HttpKernel\\Event\\GetResponseEventX;\nuse Symfony\\Component\\HttpKernel\\Event\\FilterResponseEvent as R;\n";
        let found = deprecations_in(Some((4, 3)), events);
        assert_eq!(found, [("http-kernel-event-argument", "GetResponseEvent".to_owned()), ("http-kernel-event-argument", "FilterResponseEvent".to_owned())]);
        assert!(deprecations_in(Some((4, 2)), events).is_empty());

        let security = "<?php\nuse Symfony\\Component\\Security\\Core\\Security;\nuse Symfony\\Component\\Security\\Core\\SecurityEvents;\n";
        assert_eq!(deprecations_in(Some((6, 2)), security), [("security-class-moved", "Symfony\\Component\\Security\\Core\\Security".to_owned())]);

        let request = "<?php\nuse Symfony\\Component\\HttpFoundation\\Request;\nfunction f(Request $request, $other) { $request->getContentType(); $other->getContentType(); }\n";
        assert_eq!(deprecations_in(Some((6, 2)), request), [("request-get-content-type", "getContentType".to_owned())]);

        let untyped = "<?php\nfunction f($request) { $request->getContentType(); }\n";
        assert!(deprecations_in(Some((6, 2)), untyped).is_empty());
    }

    #[test]
    fn class_model_reads_parents_interfaces_methods_and_parameters() {
        let text = "<?php\nnamespace App\\Service;\n\nuse Acme\\Base;\nuse Acme\\Contracts as C;\nuse Psr\\Log\\LoggerInterface;\n\nfinal class Mailer extends Base implements C\\Sender, \\Countable\n{\n    use Helper;\n\n    public function __construct(private LoggerInterface $logger, ?int $retries = 3, string|int $id = 1, #[Autowire('%x%')] string $dsn = '', Other ...$more)\n    {\n        $a = function () {};\n    }\n\n    private static function send(): void {}\n}\n";
        let model = class_model(text).unwrap();
        assert_eq!(model.fqcn, "App\\Service\\Mailer");
        assert_eq!(model.parent.as_deref(), Some("Acme\\Base"));
        assert_eq!(model.interfaces, ["Acme\\Contracts\\Sender", "Countable"]);
        assert!(model.has_traits);
        assert_eq!(model.methods.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["__construct", "send"]);

        let params = &model.method("__construct").unwrap().params;
        assert_eq!(params[0].type_name.as_deref(), Some("Psr\\Log\\LoggerInterface"));
        assert!(params[0].promoted);
        assert_eq!((params[1].type_name.as_deref(), params[1].nullable, params[1].has_default), (Some("int"), true, true));
        assert_eq!((params[2].type_name.as_deref(), params[2].typed), (None, true));
        assert!(params[3].has_attribute);
        assert_eq!(params[4].type_name.as_deref(), Some("App\\Service\\Other"));
        assert!(params[4].variadic);

        let send = model.method("send").unwrap();
        assert_eq!((send.visibility, send.is_static, send.return_type.as_deref()), (crate::php_text::Visibility::Private, true, Some("void")));
    }

    #[test]
    fn practice_group_is_published_and_can_be_switched_off() {
        let host = MockHost::install();
        let mut plugin = project(&host);
        host.add_file("src/Command/XCommand.php", CONTROLLER);
        assert!(published(&host, &mut plugin, Event::FileOpened(PATH.into())).contains(&"controller-superglobal".to_owned()));

        host.set_setting("practiceChecks", "false");
        assert!(published(&host, &mut plugin, Event::FileSaved(PATH.into())).is_empty());
    }

    #[test]
    fn practice_package_codes_are_published() {
        let host = MockHost::install();
        let mut plugin = project(&host);
        let text = "<?php\nnamespace App\\Command;\n\nuse Symfony\\Component\\Routing\\Annotation\\Route;\nuse Symfony\\Bundle\\FrameworkBundle\\Controller\\AbstractController;\n\nclass XCommand extends AbstractController\n{\n    public function a()\n    {\n        $this->createNotFoundException('x');\n    }\n}\n";
        host.add_file("src/Command/XCommand.php", text);

        let codes = published(&host, &mut plugin, Event::FileOpened(PATH.into()));
        assert!(codes.contains(&"controller-missing-exit".to_owned()));
        assert!(codes.contains(&"route-attribute-namespace".to_owned()));
    }
}
