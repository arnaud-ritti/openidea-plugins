//! Practice advice and later deprecations for PHP classes, written from the
//! Symfony documentation (controllers, service container, security, forms,
//! validation, console) and the UPGRADE notes. Roles come from the resolved
//! parent class, interfaces and class attributes; a check stays silent when
//! the role is not certain.

use std::ops::Range;

use crate::checks::Finding;
use crate::php_checks::{arguments, occurrences};
use crate::php_text::{attributes, imports, matching_close, read_word, skip_whitespace, ClassModel, Method, Target, Visibility};

const REQUEST: &str = "Symfony\\Component\\HttpFoundation\\Request";
const REQUEST_STACK: &str = "Symfony\\Component\\HttpFoundation\\RequestStack";
const SESSION_TYPES: [&str; 2] = ["Symfony\\Component\\HttpFoundation\\Session\\SessionInterface", "Symfony\\Component\\HttpFoundation\\Session\\Session"];
const TOKEN: &str = "Symfony\\Component\\Security\\Core\\Authentication\\Token\\TokenInterface";
const USER: &str = "Symfony\\Component\\Security\\Core\\User\\UserInterface";
const KERNEL: &str = "Symfony\\Component\\HttpKernel\\KernelInterface";
const COMMAND: &str = "Symfony\\Component\\Console\\Command\\Command";
const BUNDLE: &str = "Symfony\\Component\\HttpKernel\\Bundle\\Bundle";
const TWIG_EXTENSION: &str = "Twig\\Extension\\AbstractExtension";
const FORM_TYPE: &str = "Symfony\\Component\\Form\\AbstractType";
const CONTROLLER: &str = "Symfony\\Bundle\\FrameworkBundle\\Controller\\AbstractController";
const SERVICE_ROLE_PARENTS: [&str; 5] = [
    COMMAND,
    TWIG_EXTENSION,
    FORM_TYPE,
    "Symfony\\Component\\Security\\Core\\Authorization\\Voter\\Voter",
    "Symfony\\Component\\Validator\\ConstraintValidator",
];
const SERVICE_ROLE_INTERFACES: [&str; 2] = ["Symfony\\Component\\EventDispatcher\\EventSubscriberInterface", "Symfony\\Component\\Security\\Core\\Authorization\\Voter\\VoterInterface"];
const SERVICE_ROLE_ATTRIBUTES: [&str; 3] = [
    "Symfony\\Component\\Console\\Attribute\\AsCommand",
    "Symfony\\Component\\HttpKernel\\Attribute\\AsController",
    "Symfony\\Component\\EventDispatcher\\Attribute\\AsEventListener",
];

/// Types of the Symfony 2 to 5 era that were deprecated and removed, with their replacements.
const DEPRECATED_TYPES: [(&str, &str); 7] = [
    ("Symfony\\Component\\Security\\Core\\Encoder\\UserPasswordEncoderInterface", "Symfony\\Component\\PasswordHasher\\Hasher\\UserPasswordHasherInterface"),
    ("Symfony\\Component\\Security\\Core\\Encoder\\EncoderFactoryInterface", "Symfony\\Component\\PasswordHasher\\Hasher\\PasswordHasherFactoryInterface"),
    ("Symfony\\Component\\Templating\\EngineInterface", "Twig\\Environment"),
    ("Symfony\\Bundle\\FrameworkBundle\\Templating\\EngineInterface", "Twig\\Environment"),
    ("Symfony\\Component\\Translation\\TranslatorInterface", "Symfony\\Contracts\\Translation\\TranslatorInterface"),
    ("Doctrine\\Common\\Persistence\\ManagerRegistry", "Doctrine\\Persistence\\ManagerRegistry"),
    ("Doctrine\\Common\\Persistence\\ObjectManager", "Doctrine\\Persistence\\ObjectManager"),
];

const SHORTCUT_RESPONSES: [(&str, &str); 3] = [
    ("Symfony\\Component\\HttpFoundation\\RedirectResponse", "$this->redirect() or redirectToRoute()"),
    ("Symfony\\Component\\HttpFoundation\\JsonResponse", "$this->json()"),
    ("Symfony\\Component\\HttpFoundation\\BinaryFileResponse", "$this->file()"),
];

/// Controller helpers whose return value is the response or the exception to throw.
const RETURNING_HELPERS: [&str; 7] = ["render", "redirect", "redirectToRoute", "json", "file", "forward", "renderView"];
const THROWING_HELPERS: [&str; 2] = ["createNotFoundException", "createAccessDeniedException"];

const REQUEST_BAGS: [&str; 7] = ["attributes", "request", "query", "server", "files", "cookies", "headers"];

const VALIDATOR_CONSTRAINTS: &str = "Symfony\\Component\\Validator\\Constraints\\";
const VALIDATOR_NAMESPACE: &str = "Symfony\\Component\\Validator";

const FIRST_CLASS_CALLABLE_SINCE: (u32, u32) = (6, 2);
const RENDER_FORM_SINCE: (u32, u32) = (6, 2);
const CONSTRAINT_ARRAY_SINCE: (u32, u32) = (7, 4);
const ROUTE_ATTRIBUTE_SINCE: (u32, u32) = (6, 4);

const OLD_ROUTE: &str = "Symfony\\Component\\Routing\\Annotation\\Route";
const NEW_ROUTE: &str = "Symfony\\Component\\Routing\\Attribute\\Route";

fn is_controller(model: &ClassModel) -> bool {
    model.parent.as_deref() == Some(CONTROLLER)
}

fn is_container_built(model: &ClassModel, text: &str) -> bool {
    let by_parent = model.parent.as_deref().is_some_and(|parent| parent == CONTROLLER || SERVICE_ROLE_PARENTS.contains(&parent));
    let by_interface = model.interfaces.iter().any(|interface| SERVICE_ROLE_INTERFACES.contains(&interface.as_str()));
    by_parent || by_interface || attributes(text).iter().any(|attribute| matches!(attribute.target, Target::Class { .. }) && SERVICE_ROLE_ATTRIBUTES.contains(&attribute.name.as_str()))
}

fn constructor(model: &ClassModel) -> Option<&Method> {
    model.method("__construct")
}

fn is_class_type(name: &str) -> bool {
    !crate::php_text::BUILT_IN_TYPES.contains(&name)
}

pub fn practices(text: &str, masked: &str, model: &ClassModel) -> Vec<Finding> {
    let mut found = Vec::new();

    if is_container_built(model, text) {
        found.extend(request_scoped_injection(model));
    }

    found.extend(deprecated_injection(model));
    found.extend(project_dir(masked, model));
    found.extend(heavy_constructor(masked, model));
    found.extend(register_commands(model));
    found.extend(command_return_value(masked, model));
    found.extend(request_bag_write(masked));
    found.extend(find_by_codes(masked, text));
    found.extend(form_type_instance(masked, model));

    if is_controller(model) {
        found.extend(request_stack_in_action(model));
        found.extend(controller_shortcuts(masked, text, model));
        found.extend(missing_exit(masked, model));
    }

    found
}

pub fn deprecations(masked: &str, text: &str, model: Option<&ClassModel>, reached: &dyn Fn((u32, u32)) -> bool) -> Vec<Finding> {
    let mut found = Vec::new();

    if reached(ROUTE_ATTRIBUTE_SINCE) {
        found.extend(route_namespace(masked));
    }

    if reached(CONSTRAINT_ARRAY_SINCE) {
        found.extend(constraint_array_options(masked, text));
    }

    let Some(model) = model else { return found };

    if reached(FIRST_CLASS_CALLABLE_SINCE) {
        found.extend(first_class_callable(masked, text, model));
    }

    if reached(RENDER_FORM_SINCE) && is_controller(model) {
        found.extend(render_form(masked, model));
    }

    found
}

fn request_scoped_injection(model: &ClassModel) -> Vec<Finding> {
    let Some(constructor) = constructor(model) else { return Vec::new() };
    let mut found = Vec::new();

    for param in constructor.params.iter().filter(|param| !param.has_attribute) {
        let Some(type_name) = param.type_name.as_deref() else { continue };
        let (code, message) = match type_name {
            REQUEST => ("constructor-request-injection", "The Request is not a service: inject RequestStack and call getCurrentRequest() in the method that needs it."),
            TOKEN => ("constructor-token-injection", "The security token changes per request and is not a service: inject TokenStorageInterface and read getToken() when needed."),
            USER => ("constructor-user-injection", "The current user is not a service: inject Security and call getUser(), or use #[CurrentUser] on a controller argument."),
            name if SESSION_TYPES.contains(&name) => ("constructor-session-injection", "The session belongs to the request: inject RequestStack and call getSession() (the session service is not autowirable since Symfony 6.0)."),
            _ => continue,
        };
        found.push(Finding::hint(param.range.clone(), code, message.to_owned()));
    }

    found
}

fn deprecated_injection(model: &ClassModel) -> Vec<Finding> {
    let mut found = Vec::new();

    for method in &model.methods {
        for param in &method.params {
            let Some((old, replacement)) = param.type_name.as_deref().and_then(|name| DEPRECATED_TYPES.iter().find(|(old, _)| *old == name)) else { continue };
            let message = format!("{old} was removed in Symfony 5 or Doctrine Persistence 2: use {replacement}.");
            found.push(Finding::warn(param.range.clone(), "deprecated-injection", message));
        }
    }

    found
}

fn project_dir(masked: &str, model: &ClassModel) -> Vec<Finding> {
    let Some(constructor) = constructor(model) else { return Vec::new() };
    let Some(param) = constructor.params.iter().find(|param| param.type_name.as_deref() == Some(KERNEL)) else { return Vec::new() };

    let mut calls = Vec::new();
    let mut other_use = false;
    for receiver in [format!("${}->", param.name), format!("$this->{}->", param.name)] {
        for (at, _) in masked[model.body.clone()].match_indices(&receiver) {
            let at = model.body.start + at;
            let word = read_word(masked, at + receiver.len());
            match &masked[word.clone()] {
                "getProjectDir" => calls.push(word),
                _ => other_use = true,
            }
        }
    }

    if other_use {
        return Vec::new();
    }

    calls
        .into_iter()
        .map(|range| Finding::hint(range, "kernel-project-dir", "Inject the directory instead of the kernel: #[Autowire('%kernel.project_dir%')] string $projectDir.".to_owned()))
        .collect()
}

fn heavy_constructor(masked: &str, model: &ClassModel) -> Vec<Finding> {
    let Some(constructor) = constructor(model) else { return Vec::new() };
    let name = constructor.name_range.clone();

    match model.parent.as_deref() {
        Some(TWIG_EXTENSION) if constructor.params.iter().any(|param| param.type_name.as_deref().is_some_and(is_class_type)) => {
            let message = "Twig extensions are built on every request that renders a template: move the services into a runtime class (RuntimeExtensionInterface) so they load lazily.".to_owned();
            vec![Finding::hint(name, "twig-extension-heavy-constructor", message)]
        }
        Some(COMMAND) => {
            let Some(body) = constructor.body.clone() else { return Vec::new() };
            let works = constructor.params.iter().any(|param| {
                let receiver = format!("${}->", param.name);
                masked[body.clone()].match_indices(&receiver).any(|(at, _)| {
                    let word = read_word(masked, body.start + at + receiver.len());
                    masked[body.start + at + receiver.len() + word.len()..].starts_with('(')
                })
            });
            let message = "A command is built whenever the console starts, even to run another command: do the work in initialize() or execute().".to_owned();
            works.then(|| Finding::hint(name, "command-heavy-constructor", message)).into_iter().collect()
        }
        _ => Vec::new(),
    }
}

fn register_commands(model: &ClassModel) -> Vec<Finding> {
    let Some(method) = model.method("registerCommands").filter(|_| model.parent.as_deref() == Some(BUNDLE)) else { return Vec::new() };
    let message = "Registering commands by hand in a bundle is legacy: tag the command services with console.command or use #[AsCommand].".to_owned();
    vec![Finding::hint(method.name_range.clone(), "bundle-register-commands", message)]
}

fn command_return_value(masked: &str, model: &ClassModel) -> Vec<Finding> {
    if model.parent.as_deref() != Some(COMMAND) {
        return Vec::new();
    }

    let mut found = Vec::new();
    for name in ["execute", "__invoke"] {
        let Some(body) = model.method(name).and_then(|method| method.body.clone()) else { continue };
        if masked[body.clone()].contains("function") || masked[body.clone()].contains("fn(") || masked[body.clone()].contains("fn (") {
            continue;
        }

        for at in occurrences(masked, body.clone(), "return") {
            let end = at + "return".len();
            if masked.as_bytes().get(end).is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_') {
                continue;
            }

            let value_end = masked[end..].find(';').map_or(body.end, |semicolon| end + semicolon);
            let value = masked[end..value_end].trim();
            let wrong = value.is_empty() || matches!(value.to_ascii_lowercase().as_str(), "null" | "true" | "false") || value.starts_with(['\'', '"']);
            if wrong {
                let message = "A command must return an int: Command::SUCCESS, Command::FAILURE or Command::INVALID.".to_owned();
                found.push(Finding::warn(at..value_end.max(end), "command-return-value", message));
            }
        }
    }

    found
}

fn request_bag_write(masked: &str) -> Vec<Finding> {
    if !masked.contains("Request $request") {
        return Vec::new();
    }

    let mut found = Vec::new();
    for bag in REQUEST_BAGS {
        let needle = format!("$request->{bag}");
        for at in occurrences(masked, 0..masked.len(), &needle) {
            let end = at + needle.len();
            let after = skip_whitespace(masked, end);
            let assigns = masked[after..].starts_with('=') && !masked[after..].starts_with("==") && !masked[after..].starts_with("=>");
            if assigns && !masked.as_bytes().get(end).is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_') {
                let message = format!("Do not replace the \"{bag}\" bag of the Request: change it through its methods, for example $request->{bag}->set().");
                found.push(Finding::hint(at + "$request->".len()..end, "request-bag-write", message));
            }
        }
    }

    found
}

fn find_by_codes(masked: &str, text: &str) -> Vec<Finding> {
    if !text.contains(VALIDATOR_NAMESPACE) {
        return Vec::new();
    }

    let mut found = Vec::new();
    for (at, _) in masked.match_indices("->findByCodes") {
        let name = at + 2;
        let open = skip_whitespace(masked, name + "findByCodes".len());
        if !masked[open..].starts_with('(') {
            continue;
        }

        let Some(first) = arguments(masked, open).into_iter().next() else { continue };
        let range = crate::php_checks::trimmed(masked, first);
        let literal = text[range.clone()].starts_with(['\'', '"']);
        if literal {
            found.push(Finding::hint(range, "violations-find-by-codes", "Pass the error-code constants of the constraint (for example NotBlank::IS_BLANK_ERROR) instead of a literal code.".to_owned()));
        }
    }

    found
}

fn form_type_instance(masked: &str, model: &ClassModel) -> Vec<Finding> {
    if model.parent.as_deref() != Some(FORM_TYPE) {
        return Vec::new();
    }

    let mut found = Vec::new();
    for (at, _) in masked[model.body.clone()].match_indices("->add(") {
        let open = model.body.start + at + "->add".len();
        let Some(second) = arguments(masked, open).into_iter().nth(1) else { continue };
        let range = crate::php_checks::trimmed(masked, second);
        if masked[range.clone()].starts_with("new ") {
            found.push(Finding::warn(range, "form-type-instance", "Form types are passed as class names (TextType::class): passing an instance was removed in Symfony 3.0.".to_owned()));
        }
    }

    found
}

fn request_stack_in_action(model: &ClassModel) -> Vec<Finding> {
    model
        .methods
        .iter()
        .filter(|method| method.visibility == Visibility::Public && !method.name.starts_with("__"))
        .flat_map(|method| method.params.iter())
        .filter(|param| param.type_name.as_deref() == Some(REQUEST_STACK))
        .map(|param| Finding::hint(param.range.clone(), "request-stack-in-action", "A controller action can type-hint Request directly instead of going through RequestStack.".to_owned()))
        .collect()
}

fn controller_shortcuts(masked: &str, text: &str, model: &ClassModel) -> Vec<Finding> {
    let imports = imports(text);
    let mut found = Vec::new();

    for (fqcn, shortcut) in SHORTCUT_RESPONSES {
        let Some(import) = imports.iter().find(|import| import.fqcn == fqcn) else { continue };
        let needle = format!("new {}", import.alias);
        for (at, _) in masked[model.body.clone()].match_indices(&needle) {
            let at = model.body.start + at;
            let end = at + needle.len();
            let whole_word = !masked.as_bytes().get(end).is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_');
            let own_new = !masked[..at].bytes().last().is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'\\'));
            if whole_word && own_new {
                found.push(Finding::hint(at..end, "controller-shortcut", format!("AbstractController has a shortcut for this response: {shortcut}.")));
            }
        }
    }

    found
}

fn missing_exit(masked: &str, model: &ClassModel) -> Vec<Finding> {
    let mut found = Vec::new();

    for method in &model.methods {
        let Some(body) = method.body.clone() else { continue };
        for helper in RETURNING_HELPERS.iter().chain(THROWING_HELPERS.iter()) {
            let needle = format!("$this->{helper}(");
            for at in occurrences(masked, body.clone(), &needle) {
                let previous = masked[..at].trim_end().bytes().last();
                if !matches!(previous, Some(b';' | b'{' | b'}')) {
                    continue;
                }

                let name = at + "$this->".len();
                let message = match THROWING_HELPERS.contains(helper) {
                    true => format!("{helper}() only creates the exception: write `throw $this->{helper}(...)`."),
                    false => format!("The result of {helper}() is dropped: write `return $this->{helper}(...)`."),
                };
                found.push(Finding::warn(name..name + helper.len(), "controller-missing-exit", message));
            }
        }
    }

    found
}

fn first_class_callable(masked: &str, text: &str, model: &ClassModel) -> Vec<Finding> {
    let mut found = Vec::new();

    for (at, _) in masked[model.body.clone()].match_indices("[$this,") {
        let at = model.body.start + at;
        let Some(close) = matching_close(masked, at) else { continue };
        let name = text[at + "[$this,".len()..close].trim();
        let method = name.strip_prefix(['\'', '"']).and_then(|rest| rest.strip_suffix(['\'', '"']));
        let Some(method) = method.filter(|method| model.method(method).is_some()) else { continue };
        found.push(Finding::hint(at..close + 1, "first-class-callable", format!("Write `$this->{method}(...)`: first-class callable syntax is checked by static analysis, unlike an array callable.")));
    }

    found
}

fn render_form(masked: &str, model: &ClassModel) -> Vec<Finding> {
    let mut found = Vec::new();

    for (at, _) in masked[model.body.clone()].match_indices("$this->renderForm(") {
        let name = model.body.start + at + "$this->".len();
        found.push(Finding::hint(name..name + "renderForm".len(), "render-form", "render() renders a form given as a parameter since Symfony 6.2: call render() with the 422 status instead of renderForm().".to_owned()));
    }

    for helper in ["render", "renderView"] {
        let needle = format!("$this->{helper}(");
        for at in occurrences(masked, model.body.clone(), &needle) {
            let open = at + needle.len() - 1;
            let Some(close) = matching_close(masked, open) else { continue };
            for (call, _) in masked[open..close].match_indices("->createView()") {
                let name = open + call + 2;
                found.push(Finding::hint(name..name + "createView".len(), "form-create-view", "render() converts a form passed as a parameter to its view: pass the form itself.".to_owned()));
            }
        }
    }

    found
}

fn route_namespace(masked: &str) -> Vec<Finding> {
    let mut found = Vec::new();

    for (at, _) in masked.match_indices(OLD_ROUTE) {
        let end = at + OLD_ROUTE.len();
        let whole = !masked.as_bytes().get(end).is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'\\'));
        if whole {
            let message = "Symfony\\Component\\Routing\\Annotation\\Route is deprecated since Symfony 6.4: use Symfony\\Component\\Routing\\Attribute\\Route.".to_owned();
            found.push(Finding { replacement: Some(NEW_ROUTE.to_owned()), ..Finding::warn(at..end, "route-attribute-namespace", message) });
        }
    }

    found
}

fn constraint_array_options(masked: &str, text: &str) -> Vec<Finding> {
    let imports = imports(text);
    let mut calls: Vec<Range<usize>> = Vec::new();

    for import in imports.iter().filter(|import| import.fqcn.starts_with(VALIDATOR_CONSTRAINTS)) {
        let needle = format!("new {}", import.alias);
        for (at, _) in masked.match_indices(&needle) {
            let open = skip_whitespace(masked, at + needle.len());
            if masked[open..].starts_with('(') {
                calls.push(open..open + 1);
            }
        }
    }

    for attribute in attributes(text) {
        let constraint = attribute.name.starts_with(VALIDATOR_CONSTRAINTS);
        if constraint && masked[attribute.range.clone()].contains('(') {
            calls.push(attribute.arguments.start - 1..attribute.arguments.start);
        }
    }

    let mut found = Vec::new();
    for call in calls {
        let Some(first) = arguments(masked, call.start).into_iter().next() else { continue };
        let range = crate::php_checks::trimmed(masked, first);
        let is_array = masked[range.clone()].starts_with('[');
        let has_keys = is_array && text[range.clone()].contains("=>");
        if has_keys {
            found.push(Finding::warn(range, "constraint-array-options", "Passing an array of options to a constraint is deprecated since Symfony 7.4: use named arguments.".to_owned()));
        }
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::php_text::{class_model, mask};

    fn codes(text: &str, version: Option<(u32, u32)>) -> Vec<(&'static str, String)> {
        let masked = mask(text);
        let model = class_model(text);
        let mut found = model.as_ref().map(|model| practices(text, &masked, model)).unwrap_or_default();
        found.extend(deprecations(&masked, text, model.as_ref(), &|gate| version.is_some_and(|version| version >= gate)));
        found.sort_by_key(|finding| finding.start);
        found.into_iter().map(|finding| (finding.code, text[finding.start..finding.end].to_owned())).collect()
    }

    fn controller(body: &str) -> String {
        format!("<?php\nnamespace App\\Controller;\n\nuse Symfony\\Bundle\\FrameworkBundle\\Controller\\AbstractController;\nuse Symfony\\Component\\HttpFoundation\\RedirectResponse;\nuse Symfony\\Component\\HttpFoundation\\RequestStack;\n\nclass X extends AbstractController\n{{\n{body}\n}}\n")
    }

    #[test]
    fn request_scoped_services_in_container_built_constructors() {
        let text = "<?php\nuse Symfony\\Component\\Console\\Command\\Command;\nuse Symfony\\Component\\HttpFoundation\\Request;\nuse Symfony\\Component\\HttpFoundation\\Session\\SessionInterface;\nuse Symfony\\Component\\Security\\Core\\User\\UserInterface;\nuse Symfony\\Component\\Security\\Core\\Authentication\\Token\\TokenInterface;\n\nclass C extends Command\n{\n    public function __construct(Request $r, SessionInterface $s, UserInterface $u, TokenInterface $t) {}\n}\n";
        let found: Vec<_> = codes(text, None).into_iter().map(|(code, _)| code).collect();
        assert_eq!(found, ["constructor-request-injection", "constructor-session-injection", "constructor-user-injection", "constructor-token-injection"]);
    }

    #[test]
    fn plain_dto_taking_a_user_is_quiet() {
        let text = "<?php\nuse Symfony\\Component\\Security\\Core\\User\\UserInterface;\n\nclass Notice\n{\n    public function __construct(UserInterface $u) {}\n}\n";
        assert!(codes(text, None).is_empty());
    }

    #[test]
    fn removed_types_are_warned_anywhere() {
        let text = "<?php\nuse Symfony\\Component\\Security\\Core\\Encoder\\UserPasswordEncoderInterface;\n\nclass S\n{\n    public function __construct(UserPasswordEncoderInterface $e) {}\n}\n";
        assert_eq!(codes(text, None).iter().map(|(code, _)| *code).collect::<Vec<_>>(), ["deprecated-injection"]);
    }

    #[test]
    fn kernel_only_used_for_project_dir_is_a_hint() {
        let text = "<?php\nuse Symfony\\Component\\HttpKernel\\KernelInterface;\n\nclass S\n{\n    public function __construct(private KernelInterface $kernel) {}\n    public function dir() { return $this->kernel->getProjectDir(); }\n}\n";
        assert_eq!(codes(text, None), [("kernel-project-dir", "getProjectDir".to_owned())]);

        let used = text.replace("getProjectDir()", "getProjectDir() . $this->kernel->getEnvironment()");
        assert!(codes(&used, None).is_empty());
    }

    #[test]
    fn heavy_constructors_of_extensions_and_commands() {
        let extension = "<?php\nuse Twig\\Extension\\AbstractExtension;\nuse Doctrine\\ORM\\EntityManagerInterface;\n\nclass E extends AbstractExtension\n{\n    public function __construct(EntityManagerInterface $em) {}\n}\n";
        assert_eq!(codes(extension, None).iter().map(|(code, _)| *code).collect::<Vec<_>>(), ["twig-extension-heavy-constructor"]);

        let command = "<?php\nuse Symfony\\Component\\Console\\Command\\Command;\n\nclass C extends Command\n{\n    public function __construct(private $repo)\n    {\n        parent::__construct();\n        $repo->load();\n    }\n}\n";
        assert_eq!(codes(command, None).iter().map(|(code, _)| *code).collect::<Vec<_>>(), ["command-heavy-constructor"]);

        let light = command.replace("$repo->load();", "");
        assert!(codes(&light, None).is_empty());
    }

    #[test]
    fn bundle_register_commands_is_a_hint() {
        let text = "<?php\nuse Symfony\\Component\\HttpKernel\\Bundle\\Bundle;\n\nclass B extends Bundle\n{\n    public function registerCommands($application) {}\n}\n";
        assert_eq!(codes(text, None), [("bundle-register-commands", "registerCommands".to_owned())]);
    }

    #[test]
    fn command_must_return_an_int() {
        let text = "<?php\nuse Symfony\\Component\\Console\\Command\\Command;\n\nclass C extends Command\n{\n    protected function execute($i, $o): int\n    {\n        if ($i) {\n            return;\n        }\n        return Command::SUCCESS;\n    }\n}\n";
        assert_eq!(codes(text, None), [("command-return-value", "return".to_owned())]);

        let closure = text.replace("if ($i) {", "$f = function () { return null; }; if ($i) {");
        assert!(codes(&closure, None).is_empty());
    }

    #[test]
    fn replacing_a_request_bag_is_a_hint() {
        let text = "<?php\nuse Symfony\\Component\\HttpFoundation\\Request;\n\nclass A\n{\n    function f(Request $request) {\n    $request->attributes = new ParameterBag();\n    $request->attributes->set('a', 1);\n    if ($request->query == 1) {}\n}\n}\n";
        assert_eq!(codes(text, None), [("request-bag-write", "attributes".to_owned())]);
    }

    #[test]
    fn find_by_codes_with_a_literal_code() {
        let text = "<?php\nuse Symfony\\Component\\Validator\\ConstraintViolationList;\n\nclass A\n{\n    function f($l) {\n$l->findByCodes('c1051bb4-d103-4f74-8988-acbcafc7fdc3');\n$l->findByCodes(NotBlank::IS_BLANK_ERROR);\n}\n}\n";
        assert_eq!(codes(text, None), [("violations-find-by-codes", "'c1051bb4-d103-4f74-8988-acbcafc7fdc3'".to_owned())]);
    }

    #[test]
    fn form_type_instances_are_warned() {
        let text = "<?php\nuse Symfony\\Component\\Form\\AbstractType;\n\nclass F extends AbstractType\n{\n    public function buildForm($builder, array $o)\n    {\n        $builder->add('a', new TextType())->add('b', TextType::class);\n    }\n}\n";
        assert_eq!(codes(text, None), [("form-type-instance", "new TextType()".to_owned())]);
    }

    #[test]
    fn controller_action_checks() {
        let text = controller("    public function a(RequestStack $stack)\n    {\n        $this->createNotFoundException('x');\n        $this->redirectToRoute('r');\n        throw $this->createNotFoundException('y');\n        return new RedirectResponse('/');\n    }");
        let found: Vec<_> = codes(&text, None).into_iter().collect();
        assert_eq!(
            found,
            [
                ("request-stack-in-action", "RequestStack $stack".to_owned()),
                ("controller-missing-exit", "createNotFoundException".to_owned()),
                ("controller-missing-exit", "redirectToRoute".to_owned()),
                ("controller-shortcut", "new RedirectResponse".to_owned()),
            ]
        );
    }

    #[test]
    fn first_class_callable_and_form_rendering_follow_the_version() {
        let text = controller("    public function a()\n    {\n        $f = [$this, 'a'];\n        $g = [$this, 'zzz'];\n        return $this->renderForm('t', ['form' => $form]);\n    }");
        assert!(codes(&text, Some((6, 1))).is_empty());
        let found: Vec<_> = codes(&text, Some((6, 4))).into_iter().map(|(code, _)| code).collect();
        assert_eq!(found, ["first-class-callable", "render-form"]);

        let view = controller("    public function a()\n    {\n        return $this->render('t', ['form' => $form->createView()]);\n    }");
        assert_eq!(codes(&view, Some((6, 4))), [("form-create-view", "createView".to_owned())]);
    }

    #[test]
    fn route_annotation_namespace_has_a_replacement() {
        let text = "<?php\nuse Symfony\\Component\\Routing\\Annotation\\Route;\n";
        assert!(codes(text, Some((6, 3))).is_empty());
        assert_eq!(codes(text, Some((6, 4))), [("route-attribute-namespace", "Symfony\\Component\\Routing\\Annotation\\Route".to_owned())]);
    }

    #[test]
    fn constraint_option_arrays_are_deprecated_since_7_4() {
        let text = "<?php\nuse Symfony\\Component\\Validator\\Constraints\\NotBlank;\nuse Symfony\\Component\\Validator\\Constraints as Assert;\n\n$a = new NotBlank(['message' => 'x']);\n$b = new NotBlank(message: 'x');\n$c = new Choice(['a', 'b']);\n\nclass E\n{\n    #[Assert\\NotBlank(['message' => 'x'])]\n    public $p;\n}\n";
        assert!(codes(text, Some((7, 3))).is_empty());
        assert_eq!(codes(text, Some((7, 4))).iter().map(|(code, _)| *code).collect::<Vec<_>>(), ["constraint-array-options"; 2].to_vec());
    }
}
