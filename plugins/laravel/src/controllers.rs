//! Controller classes of `app/Http/Controllers` and their public methods.

use crate::php::is_identifier;

pub struct Controller {
    /// Relative to `App\Http\Controllers`: `Auth\LoginController`.
    pub class: String,
    /// Offset of the class name.
    pub class_offset: usize,
    /// Public instance methods and the offset of each name.
    pub methods: Vec<(String, usize)>,
}

/// The class a controller file declares, from its text with comments blanked.
pub fn parse(relative: &str, text: &str) -> Option<Controller> {
    let class = relative.strip_suffix(".php")?.replace('/', "\\");
    let short = class.rsplit('\\').next()?;
    let declaration = text.match_indices("class ").find_map(|(at, _)| {
        let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
        let before = text[line_start..at].trim();
        let name = &text[at + "class ".len()..];
        let declares = before.split_whitespace().all(|word| matches!(word, "final" | "readonly"));
        (declares && name.starts_with(short)).then_some(at + "class ".len())
    })?;

    let mut methods = Vec::new();
    for (at, _) in text[declaration..].match_indices("function ") {
        let at = declaration + at;
        let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
        let modifiers = &text[line_start..at];
        if modifiers.contains("private") || modifiers.contains("protected") || modifiers.contains("static") {
            continue;
        }

        let name_start = at + "function ".len();
        let name_length = text[name_start..].bytes().position(|b| !is_identifier(b)).unwrap_or(0);
        let name = &text[name_start..name_start + name_length];
        if !name.is_empty() && !name.starts_with("__") {
            methods.push((name.to_owned(), name_start));
        }
    }

    Some(Controller { class, class_offset: declaration, methods })
}

/// Every method a controller file declares, whatever its visibility, or
/// `None` when the class may have more (a trait, `__call`, a parent other
/// than `Controller`).
pub fn declared_methods(masked: &str) -> Option<Vec<String>> {
    let class_at = masked.match_indices("class ").find_map(|(at, _)| {
        let line_start = masked[..at].rfind('\n').map_or(0, |i| i + 1);
        masked[line_start..at].split_whitespace().all(|word| matches!(word, "final" | "abstract" | "readonly")).then_some(at)
    })?;
    let open = masked[class_at..].find('{').map(|i| class_at + i)?;

    let header: Vec<&str> = masked[class_at..open].split_whitespace().collect();
    if let Some(at) = header.iter().position(|word| *word == "extends") {
        let parent = header.get(at + 1)?.trim_start_matches('\\');
        if parent.rsplit('\\').next() != Some("Controller") {
            return None;
        }
    }

    let body = &masked[open..];
    let has_trait = body.lines().skip(1).map(str::trim).any(|line| line.starts_with("use ") && !line.contains('(') && line.ends_with(';'));
    if has_trait || body.contains("__call") {
        return None;
    }

    let names = body
        .match_indices("function ")
        .map(|(at, _)| body[at + "function ".len()..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect::<String>())
        .filter(|name| !name.is_empty())
        .collect();
    Some(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_public_methods_only() {
        let text = "<?php\nnamespace App\\Http\\Controllers\\Auth;\nfinal class LoginController extends Controller\n{\n    public function login() { $f = function () {}; }\n    protected function guard() {}\n    private static function x() {}\n    function show() {}\n    public function __construct() {}\n}\n";
        let controller = parse("Auth/LoginController.php", text).unwrap();
        assert_eq!(controller.class, "Auth\\LoginController");
        let methods: Vec<_> = controller.methods.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(methods, ["login", "show"]);
        assert_eq!(&text[controller.class_offset..controller.class_offset + 15], "LoginController");
    }

    #[test]
    fn ignores_files_that_declare_no_such_class() {
        assert!(parse("A.php", "<?php\ntrait A {}\n").is_none());
    }

    #[test]
    fn declared_methods_are_unknown_for_classes_that_may_inherit_more() {
        let plain = "<?php\nclass A extends Controller\n{\n    public function index() {}\n    protected function p() {}\n}\n";
        assert_eq!(declared_methods(plain).unwrap(), ["index", "p"]);
        assert!(declared_methods("<?php\nclass A extends Base\n{\n}\n").is_none());
        assert!(declared_methods("<?php\nclass A extends Controller\n{\n    use Foo;\n}\n").is_none());
        assert!(declared_methods("<?php\nclass A\n{\n    public function __call($m, $a) {}\n}\n").is_none());
    }
}
