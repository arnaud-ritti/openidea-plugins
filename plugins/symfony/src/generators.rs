//! Generators that write new code from existing definitions: a translation key
//! from selected Twig text, and form fields from the entity of a form type.
//! Doctrine column types map to form types through a static table from the
//! Symfony forms documentation. Properties are read as text.

use std::ops::Range;

use crate::checks::Context;
use crate::intentions::{eol_of, find_class, import_for, indent_at, methods_of, Action, Edit};
use crate::php_text::{class_model, mask, matching_close, read_word, resolver, ClassModel};
use crate::refs;
use crate::twig::code_regions;

const ABSTRACT_FORM_TYPE: &str = "Symfony\\Component\\Form\\AbstractType";
const FORM_TYPE_NAMESPACE: &str = "Symfony\\Component\\Form\\Extension\\Core\\Type\\";
const RELATIONS: [&str; 6] = ["OneToMany", "ManyToOne", "ManyToMany", "OneToOne", "Embedded", "Id"];

/// (Doctrine column types or PHP types, form type).
const FORM_TYPES: [(&[&str], &str); 8] = [
    (&["string", "ascii_string", "guid"], "TextType"),
    (&["text"], "TextareaType"),
    (&["integer", "smallint", "bigint", "int"], "IntegerType"),
    (&["boolean", "bool"], "CheckboxType"),
    (&["float", "decimal", "smallfloat"], "NumberType"),
    (&["date", "date_immutable"], "DateType"),
    (&["datetime", "datetime_immutable", "datetimetz", "datetimetz_immutable", "datetime", "datetimeimmutable", "datetimeinterface"], "DateTimeType"),
    (&["time", "time_immutable"], "TimeType"),
];

pub(crate) struct Property {
    pub name: String,
    pub form_type: Option<&'static str>,
}

fn form_type_for(kind: &str) -> Option<&'static str> {
    let kind = kind.to_ascii_lowercase();
    let kind = kind.rsplit(['\\', ':']).next().unwrap_or(&kind);
    FORM_TYPES.iter().find(|(kinds, _)| kinds.contains(&kind)).map(|(_, form_type)| *form_type)
}

/// Properties of an entity that become form fields, in declaration order.
/// With Doctrine attributes only `Column` properties count; without any, typed properties do.
pub(crate) fn entity_properties(source: &str) -> Vec<Property> {
    let Some(model) = class_model(source) else { return Vec::new() };
    let masked = mask(source);
    let mut statements: Vec<(String, usize, usize)> = Vec::new();
    let mut depth = 0usize;
    let mut start = model.body.start + 1;

    for at in model.body.start + 1..model.body.end {
        match masked.as_bytes()[at] {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && masked.as_bytes()[at] == b'}' {
                    start = at + 1;
                }
            }
            b';' if depth == 0 => {
                statements.push((source[start..at].to_owned(), start, at));
                start = at + 1;
            }
            _ => {}
        }
    }

    let uses_attributes = statements.iter().any(|(statement, ..)| statement.contains("Column"));
    let mut found = Vec::new();

    for (statement, begin, finish) in statements {
        let masked_statement = &masked[begin..finish];
        if masked_statement.contains("function") || masked_statement.contains("const ") || masked_statement.trim_start().starts_with("use ") {
            continue;
        }

        let Some(dollar) = masked_statement.find('$') else { continue };
        let name = read_word(&masked, begin + dollar + 1);
        let property = &source[name.clone()];
        let is_static = masked_statement[..dollar].split_whitespace().any(|word| word == "static");
        let visible = masked_statement[..dollar].split_whitespace().any(|word| matches!(word, "private" | "protected" | "public"));
        if is_static || !visible || property.is_empty() || property == "id" {
            continue;
        }

        let attribute_text = &statement[..dollar];
        let short_names: Vec<&str> = attribute_text
            .split("#[")
            .skip(1)
            .flat_map(|chunk| chunk.split([',', ']', '(']).take(1))
            .map(|name| name.trim().rsplit('\\').next().unwrap_or(name.trim()))
            .collect();
        if short_names.iter().any(|name| RELATIONS.contains(name)) {
            continue;
        }

        let has_column = short_names.contains(&"Column");
        if uses_attributes && !has_column {
            continue;
        }

        let column_type = has_column.then(|| column_type(attribute_text)).flatten();
        let declared = masked[begin..begin + dollar].rsplit(']').next().unwrap_or("");
        let declared: String = declared.split_whitespace().filter(|word| !matches!(*word, "private" | "protected" | "public" | "readonly" | "static" | "var")).collect::<Vec<_>>().join("");
        let declared = declared.trim_start_matches('?').trim_start_matches('\\').to_owned();

        let form_type = column_type.as_deref().and_then(form_type_for).or_else(|| form_type_for(&declared).filter(|_| !declared.contains('|')));
        found.push(Property { name: property.to_owned(), form_type });
    }

    found
}

fn column_type(attributes: &str) -> Option<String> {
    let at = attributes.find("Column")?;
    let rest = &attributes[at..];
    let close = rest.find(']').unwrap_or(rest.len());
    let list = &rest[..close];
    let named = list.find("type:").map(|found| &list[found + 5..]);
    let value = named?.trim_start();
    let end = value.find([',', ')']).unwrap_or(value.len());
    Some(value[..end].trim().trim_matches(['\'', '"']).to_owned())
}

fn data_class(text: &str, masked: &str, model: &ClassModel) -> Option<String> {
    let options = model.method("configureOptions")?;
    let open = masked[options.parameters.end..model.body.end].find('{')? + options.parameters.end;
    let close = matching_close(masked, open)?;
    let at = text[open..close].find("data_class")? + open;
    let rest = &text[at..close];
    let arrow = rest.find("=>")?;
    let value = rest[arrow + 2..].trim_start();
    let class = value.split("::class").next().filter(|class| value.contains("::class") && class.bytes().all(|b| refs::is_identifier(b) || b == b'\\'))?;
    Some(resolver(text)(class.trim()))
}

/// Statements `$builder->add('field', Type::class);` for the entity's
/// properties the form lacks, appended to `buildForm()`.
pub fn form_fields(text: &str, at: usize, ctx: &Context) -> Option<Action> {
    let model = class_model(text)?;
    if model.parent.as_deref() != Some(ABSTRACT_FORM_TYPE) {
        return None;
    }

    let masked = mask(text);
    let class = find_class(&masked)?;
    let methods = methods_of(text, &masked, &class);
    let build = methods.iter().find(|method| method.name == "buildForm")?;
    let header = masked[..model.name_range.start].rfind('\n').map_or(0, |found| found + 1)..masked[model.name_range.start..].find('\n').map_or(masked.len(), |found| model.name_range.start + found);
    let inside = build.span.contains(&at) || build.span.end == at || header.contains(&at) || header.end == at;
    if !inside {
        return None;
    }

    let entity = data_class(text, &masked, &model)?;
    let entry = ctx.index.classes.get(&entity)?;
    let properties = entity_properties(&ctx.source(&entry.path)?);

    let open = masked[build.parameters.end..build.span.end].find('{')? + build.parameters.end;
    let close = matching_close(&masked, open)?;
    let builder = text[build.parameters.start..build.parameters.end].split_whitespace().find(|word| word.starts_with('$') && !word.starts_with("$options"))?.trim_matches([',', ')']).to_owned();

    let mut imports: Vec<Edit> = Vec::new();
    let mut lines = Vec::new();
    let eol = eol_of(text);
    let body = format!("{}    ", indent_at(text, build.span.start));

    for property in properties {
        let Some(form_type) = property.form_type else { continue };
        let added = [format!("add('{}'", property.name), format!("add(\"{}\"", property.name)].iter().any(|needle| text.contains(needle.as_str()));
        if added {
            continue;
        }

        let fqcn = format!("{FORM_TYPE_NAMESPACE}{form_type}");
        let Some((import, short)) = import_for(text, &fqcn) else { continue };
        if short != form_type {
            continue;
        }
        if let Some(import) = import.filter(|import| !imports.iter().any(|known| known.1 == import.1)) {
            imports.push(import);
        }

        lines.push(format!("{body}{builder}->add('{}', {short}::class);{eol}", property.name));
    }

    if lines.is_empty() {
        return None;
    }

    let line_before = masked[..close].rfind('\n').map_or(0, |found| found + 1);
    let mut edits: Vec<Edit> = Vec::new();
    if let Some((range, first)) = imports.first().cloned() {
        let rest: String = imports.iter().skip(1).map(|(_, line)| line.trim_start_matches(['\r', '\n'])).collect();
        edits.push((range, format!("{first}{rest}")));
    }
    edits.push((line_before..line_before, lines.concat()));

    Some(Action { title: format!("Add form fields for {}", entity.rsplit('\\').next().unwrap_or(&entity)), kind: "refactor.rewrite", edits })
}

fn in_twig_code(text: &str, at: usize) -> bool {
    let before = &text[..at];
    let unclosed = |open: &str, close: &str| before.rfind(open).is_some_and(|opened| before.rfind(close).is_none_or(|closed| closed < opened));
    !(unclosed("{{", "}}") || unclosed("{%", "%}") || unclosed("{#", "#}"))
}

/// Text of a selection that can become a translation: plain words on one line, not inside a Twig tag.
pub(crate) fn translatable(text: &str, start: usize, end: usize) -> Option<Range<usize>> {
    let slice = &text[start..end];
    let lead = slice.len() - slice.trim_start().len();
    let range = start + lead..start + lead + slice.trim().len();
    let value = &text[range.clone()];

    let plain = !value.is_empty() && value.len() <= 200 && !value.contains(['\n', '\r', '{', '}', '<', '>', '|']);
    let in_region = code_regions(text).iter().any(|region| region.start <= range.start && range.end <= region.end);
    if !plain || !in_region || !in_twig_code(text, range.start) {
        return None;
    }

    let before = &text[..range.start];
    let inside_html_tag = before.rfind('<') > before.rfind('>');
    let quoted = before.ends_with(['"', '\'']) && text[range.end..].starts_with(before.chars().last().unwrap_or('"'));
    (!inside_html_tag || quoted).then_some(range)
}

/// `blog.index.read_more` for "Read more" in `blog/index.html.twig`.
pub(crate) fn translation_key(template: Option<&str>, value: &str) -> Option<String> {
    let words: Vec<String> = value.split(|c: char| !c.is_alphanumeric()).filter(|word| !word.is_empty()).take(6).map(str::to_lowercase).collect();
    let slug: String = words.join("_").chars().take(40).collect();
    if slug.is_empty() {
        return None;
    }

    let prefix = template
        .map(|name| name.trim_start_matches('@').trim_end_matches(".twig").trim_end_matches(".html").replace(['/', '_'], "."))
        .filter(|prefix| !prefix.is_empty())
        .map(|prefix| prefix.trim_matches('.').to_owned());

    Some(match prefix {
        Some(prefix) => format!("{prefix}.{slug}"),
        None => slug,
    })
}

pub(crate) fn default_domain(text: &str) -> String {
    let Some(at) = text.find("trans_default_domain") else { return "messages".into() };
    let rest = text[at + "trans_default_domain".len()..].trim_start();
    let quote = rest.chars().next().filter(|c| matches!(c, '\'' | '"'));
    quote.and_then(|quote| rest[1..].split(quote).next()).map(str::to_owned).unwrap_or_else(|| "messages".into())
}

pub(crate) fn translation_line(file_text: &str, key: &str, value: &str) -> (usize, String) {
    let end = file_text.len();
    let separator = if file_text.is_empty() || file_text.ends_with('\n') { "" } else { "\n" };
    let eol = if file_text.contains("\r\n") { "\r\n" } else { "\n" };
    (end, format!("{separator}'{}': '{}'{eol}", key.replace('\'', "''"), value.replace('\'', "''")))
}

#[cfg(test)]
mod tests {
    use openidea_plugin::Position;

    use super::*;
    use crate::index::{Entry, Index};

    const ENTITY: &str = "<?php\nnamespace App\\Entity;\n\nuse Doctrine\\ORM\\Mapping as ORM;\n\n#[ORM\\Entity]\nclass Product\n{\n    #[ORM\\Id, ORM\\GeneratedValue, ORM\\Column]\n    private ?int $id = null;\n\n    #[ORM\\Column(length: 255)]\n    private string $name;\n\n    #[ORM\\Column(type: 'text', nullable: true)]\n    private ?string $description = null;\n\n    #[ORM\\Column(type: Types::DATETIME_IMMUTABLE)]\n    private \\DateTimeImmutable $createdAt;\n\n    #[ORM\\Column]\n    private float $price = 0.0;\n\n    #[ORM\\Column(type: 'json')]\n    private array $tags = [];\n\n    #[ORM\\ManyToOne(targetEntity: Category::class)]\n    private ?Category $category = null;\n\n    private string $transient = '';\n\n    public static int $count = 0;\n\n    public function getName(): string\n    {\n        return $this->name;\n    }\n}\n";
    const FORM: &str = "<?php\nnamespace App\\Form;\n\nuse App\\Entity\\Product;\nuse Symfony\\Component\\Form\\AbstractType;\nuse Symfony\\Component\\Form\\FormBuilderInterface;\nuse Symfony\\Component\\OptionsResolver\\OptionsResolver;\n\nclass ProductType extends AbstractType\n{\n    public function buildForm(FormBuilderInterface $builder, array $options): void\n    {\n        $builder->add('name', TextType::class);\n    }\n\n    public function configureOptions(OptionsResolver $resolver): void\n    {\n        $resolver->setDefaults(['data_class' => Product::class]);\n    }\n}\n";

    fn index() -> Index {
        let mut index = Index::default();
        index.classes.insert("App\\Entity\\Product".into(), Entry { detail: None, path: "/project/src/Entity/Product.php".into(), at: Position::new(0, 0) });
        index
    }

    fn apply(text: &str, action: &Action) -> String {
        let mut edits: Vec<&Edit> = action.edits.iter().collect();
        edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        let mut out = text.to_owned();
        for (range, replacement) in edits {
            out.replace_range(range.clone(), replacement);
        }
        out
    }

    fn generate(form: &str, entity: &str, at: usize) -> Option<Action> {
        let index = index();
        let read = |path: &str| (path == "/project/src/Entity/Product.php").then(|| entity.to_owned());
        form_fields(form, at, &Context::new(&index, &read))
    }

    #[test]
    fn entity_columns_map_to_form_types_and_skip_ids_relations_and_statics() {
        let found: Vec<(String, Option<&str>)> = entity_properties(ENTITY).into_iter().map(|property| (property.name, property.form_type)).collect();
        assert_eq!(
            found,
            [
                ("name".to_owned(), Some("TextType")),
                ("description".to_owned(), Some("TextareaType")),
                ("createdAt".to_owned(), Some("DateTimeType")),
                ("price".to_owned(), Some("NumberType")),
                ("tags".to_owned(), None),
            ]
        );
    }

    #[test]
    fn typed_properties_count_when_the_entity_has_no_mapping_attributes() {
        let plain = "<?php\nclass Dto\n{\n    public string $title;\n    public ?int $count = null;\n    protected bool $flag = false;\n    private $untyped;\n}\n";
        let found: Vec<_> = entity_properties(plain).into_iter().map(|property| (property.name, property.form_type)).collect();
        assert_eq!(found, [("title".to_owned(), Some("TextType")), ("count".to_owned(), Some("IntegerType")), ("flag".to_owned(), Some("CheckboxType")), ("untyped".to_owned(), None)]);
    }

    #[test]
    fn missing_fields_are_appended_to_build_form_with_imports() {
        let action = generate(FORM, ENTITY, FORM.find("$builder->add").unwrap()).unwrap();
        assert_eq!(action.title, "Add form fields for Product");
        let out = apply(FORM, &action);
        let expected = "        $builder->add('name', TextType::class);\n        $builder->add('description', TextareaType::class);\n        $builder->add('createdAt', DateTimeType::class);\n        $builder->add('price', NumberType::class);\n    }\n";
        assert!(out.contains(expected), "{out}");
        assert!(out.contains("use Symfony\\Component\\Form\\Extension\\Core\\Type\\TextareaType;") && !out.contains("use Symfony\\Component\\Form\\Extension\\Core\\Type\\TextType;"), "{out}");
    }

    #[test]
    fn form_fields_are_quiet_without_data_class_missing_entity_or_new_fields() {
        let at = FORM.find("$builder->add").unwrap();
        assert!(generate(&FORM.replace("'data_class' => Product::class", "'x' => 1"), ENTITY, at).is_none());
        assert!(generate(&FORM.replace("Product::class", "Gone::class"), ENTITY, at).is_none());
        assert!(generate(&FORM.replace("extends AbstractType", "extends Other"), ENTITY, at).is_none());
        assert!(generate(FORM, ENTITY, FORM.find("setDefaults").unwrap()).is_none());
        assert!(generate(FORM, "<?php\nclass Product { #[ORM\\Column] private string $name; }\n", at).is_none());
    }

    #[test]
    fn selection_must_be_plain_text_outside_twig_tags() {
        let text = "<p>Read more</p>{{ 'x' }}<a title=\"Go home\">{% if a %}Hi{% endif %}</a>";
        let word = |value: &str| {
            let start = text.find(value).unwrap();
            translatable(text, start, start + value.len()).map(|range| text[range].to_owned())
        };
        assert_eq!(word("Read more"), Some("Read more".into()));
        assert_eq!(word("Go home"), Some("Go home".into()));
        assert_eq!(word("Hi"), Some("Hi".into()));
        assert_eq!(word("'x'"), None);
        assert_eq!(word("{{ 'x' }}"), None);
        assert_eq!(word("p>Read"), None);
        assert_eq!(translatable("{# note #} a {# b more #}", 17, 23), None);
        assert_eq!(translatable("{% verbatim %}raw{% endverbatim %}", 14, 17), None);
        assert_eq!(translatable("  ", 0, 2), None);
    }

    #[test]
    fn translation_keys_follow_the_template_and_the_words() {
        assert_eq!(translation_key(Some("blog/index.html.twig"), "Read more!"), Some("blog.index.read_more".into()));
        assert_eq!(translation_key(Some("@Twig/Exception/error.html.twig"), "Oops"), Some("Twig.Exception.error.oops".into()));
        assert_eq!(translation_key(None, "Où êtes-vous ?"), Some("où_êtes_vous".into()));
        assert_eq!(translation_key(None, "?!"), None);
        assert_eq!(translation_key(None, "one two three four five six seven"), Some("one_two_three_four_five_six".into()));
        assert_eq!(default_domain("{% trans_default_domain 'admin' %}"), "admin");
        assert_eq!(default_domain("x"), "messages");
        assert_eq!(translation_line("a: b", "k", "it's").1, "\n'k': 'it''s'\n");
        assert_eq!(translation_line("a: b\r\n", "k", "v").1, "'k': 'v'\r\n");
    }
}
