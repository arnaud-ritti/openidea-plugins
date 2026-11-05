//! Spring Data repositories: the entity they manage, and derived query
//! methods (`findByCustomerNameAndTotalGreaterThan`) read against its
//! properties.

use crate::model::{
    Edit, annotations, capitalize, closing, decapitalize, has_word, import_edit, is_ident, mask, members,
    split_top_level, without_annotations, words,
};

/// A property a query path can name; `target` is the class a path continues into.
#[derive(Clone, Debug, PartialEq)]
pub struct Prop {
    pub name: String,
    pub ty: String,
    /// Where `ty` is declared, when it needs an import.
    pub fqcn: Option<String>,
    pub target: Option<String>,
}

/// The properties of a class, `None` when it isn't known.
pub type Props<'a> = &'a dyn Fn(&str) -> Option<Vec<Prop>>;

pub struct Method {
    pub name: String,
    pub start: usize,
    pub end: usize,
    /// Declared with `@Query` (or similar): its name isn't parsed.
    pub declared_query: bool,
}

pub struct Repository {
    pub entity: String,
    pub id_type: String,
    pub id_start: usize,
    pub id_end: usize,
    pub methods: Vec<Method>,
}

/// The interface extending a `…Repository<Entity, Id>` in `text`.
pub fn parse_repository(text: &str, kotlin: bool) -> Option<Repository> {
    let masked = mask(text);
    let b = masked.as_bytes();
    let interface = words(&masked, "interface").next()?;
    let header_end = masked[interface..].find('{').map_or(masked.len(), |p| p + interface);
    let header = &masked[..header_end];

    let open = header[interface..]
        .match_indices("Repository")
        .map(|(i, _)| i + interface + "Repository".len())
        .map(|after| after + header[after..].len() - header[after..].trim_start().len())
        .find(|&after| b.get(after) == Some(&b'<'))?;

    let mut depth = 0;
    let close = (open..header_end).find(|&i| {
        match b[i] {
            b'<' => depth += 1,
            b'>' => depth -= 1,
            _ => {}
        }
        depth == 0
    })?;

    let args = split_top_level(&masked[open + 1..close]);
    let [(_, entity), (id_offset, id)] = args.as_slice() else { return None };
    let id_start = open + 1 + id_offset + (id.len() - id.trim_start().len());

    let mut methods = Vec::new();
    if header_end < masked.len() {
        let body_end = closing(b, header_end).unwrap_or(masked.len());
        let anns = annotations(&masked);

        for (start, end) in members(&masked, header_end + 1, body_end, kotlin, false) {
            let decl = without_annotations(&masked, start, end);
            if decl.contains('{') {
                continue;
            }

            let Some(paren) = decl.find('(') else { continue };
            if kotlin && !has_word(&decl, "fun") {
                continue;
            }

            let before = decl[..paren].trim_end();
            let name_len = before.bytes().rev().take_while(|&c| is_ident(c)).count();
            if name_len == 0 {
                continue;
            }

            let name_start = start + before.len() - name_len;
            let declared_query = anns
                .iter()
                .any(|a| a.start >= start && a.start < end && matches!(a.name, "Query" | "NativeQuery" | "Procedure"));
            methods.push(Method {
                name: masked[name_start..name_start + name_len].to_owned(),
                start: name_start,
                end: name_start + name_len,
                declared_query,
            });
        }
    }

    Some(Repository {
        entity: entity.trim().rsplit('.').next()?.to_owned(),
        id_type: id.trim().trim_end_matches('?').to_owned(),
        id_start,
        id_end: id_start + id.trim().len(),
        methods,
    })
}

const VERBS: [&str; 10] = ["find", "read", "get", "query", "search", "stream", "count", "exists", "delete", "remove"];

const OPERATORS: [&str; 59] = [
    "IsBetween", "Between", "IsNotNull", "NotNull", "IsNull", "Null", "IsLessThanEqual", "LessThanEqual", "IsLessThan",
    "LessThan", "IsGreaterThanEqual", "GreaterThanEqual", "IsGreaterThan", "GreaterThan", "IsBefore", "Before", "IsAfter",
    "After", "IsNotLike", "NotLike", "IsLike", "Like", "IsStartingWith", "StartingWith", "StartsWith", "IsEndingWith",
    "EndingWith", "EndsWith", "IsNotEmpty", "NotEmpty", "IsEmpty", "Empty", "IsNotContaining", "NotContaining",
    "IsContaining", "Containing", "Contains", "IsNotIn", "NotIn", "IsIn", "In", "IsTrue", "True", "IsFalse", "False",
    "IsNot", "Not", "Is", "Equals", "IgnoreCase", "IgnoringCase", "MatchesRegex", "Matches", "Regex", "Exists", "IsNear",
    "Near", "IsWithin", "Within",
];

/// The operators offered in completion.
const SUGGESTED: [&str; 18] = [
    "IgnoreCase", "Containing", "ContainingIgnoreCase", "StartingWith", "EndingWith", "Like", "Not", "In", "NotIn",
    "Between", "LessThan", "LessThanEqual", "GreaterThan", "GreaterThanEqual", "Before", "After", "IsNull", "IsNotNull",
];

const NO_ARGUMENT: [&str; 13] = [
    "IsNull", "Null", "IsNotNull", "NotNull", "IsEmpty", "Empty", "IsNotEmpty", "NotEmpty", "IsTrue", "True", "IsFalse",
    "False", "Exists",
];

/// `s` starts with `keyword` followed by the end or a new word.
fn starts_keyword(s: &str, keyword: &str) -> bool {
    s.starts_with(keyword) && s[keyword.len()..].chars().next().is_none_or(|c| c.is_uppercase() || c == '_')
}

fn longest_keyword(s: &str, keywords: &[&'static str]) -> Option<&'static str> {
    keywords.iter().copied().filter(|k| starts_keyword(s, k)).max_by_key(|k| k.len())
}

/// `(verb, offset of the predicate)`: `findFirstByName` → `("find", 11)`.
pub fn split_subject(name: &str) -> Option<(&'static str, usize)> {
    let verb = VERBS.iter().copied().find(|v| starts_keyword(name, v))?;
    let by = (verb.len()..name.len()).find(|&i| starts_keyword(&name[i..], "By"))?;
    Some((verb, by + 2))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    Property,
    /// After a property: an operator, `And`, `Or`, `OrderBy` or the end.
    Continuation,
    OrderProperty,
    /// After an ordering property: `Asc` or `Desc`.
    Direction,
}

#[derive(Debug)]
pub struct Clause {
    pub path: Vec<Prop>,
    pub operator: String,
}

#[derive(Debug)]
pub struct Parsed {
    pub clauses: Vec<Clause>,
    pub ordered: bool,
    /// Bytes of the predicate that parsed.
    pub consumed: usize,
    pub expect: Expect,
    /// A path went through a class that isn't known: nothing after it was checked.
    pub unverified: bool,
}

impl Parsed {
    pub fn complete(&self, predicate: &str) -> bool {
        self.consumed == predicate.len()
            && !self.unverified
            && match self.expect {
                Expect::Continuation | Expect::Direction => true,
                Expect::OrderProperty => self.ordered && !predicate.ends_with("OrderBy"),
                Expect::Property => false,
            }
    }
}

/// The longest property path at the start of `s`, its length, and whether it left known classes.
fn path(s: &str, class: &str, props: Props, depth: usize) -> Option<(Vec<Prop>, usize, bool)> {
    let Some(list) = props(class) else { return Some((Vec::new(), s.len(), true)) };
    let mut candidates: Vec<&Prop> = list.iter().filter(|p| starts_keyword(s, &capitalize(&p.name))).collect();
    candidates.sort_by_key(|p| std::cmp::Reverse(p.name.len()));
    let prop = candidates.first()?;
    let after = &s[prop.name.len()..];

    let continues = after.is_empty()
        || longest_keyword(after, &OPERATORS).is_some()
        || ["And", "Or", "OrderBy", "AllIgnoreCase", "AllIgnoringCase", "Asc", "Desc"].iter().any(|k| starts_keyword(after, k));

    if let Some(target) = prop.target.as_deref().filter(|_| depth < 3 && (!continues || after.starts_with('_'))) {
        let skip = usize::from(after.starts_with('_'));
        if let Some((rest, len, unverified)) = path(&after[skip..], target, props, depth + 1) {
            let mut full = vec![(*prop).clone()];
            full.extend(rest);
            return Some((full, prop.name.len() + skip + len, unverified));
        }
    }

    Some((vec![(*prop).clone()], prop.name.len(), false))
}

pub fn parse(predicate: &str, entity: &str, props: Props) -> Parsed {
    let mut parsed =
        Parsed { clauses: Vec::new(), ordered: false, consumed: 0, expect: Expect::Property, unverified: false };
    let mut pos = 0;

    while pos < predicate.len() && !parsed.unverified {
        let rest = &predicate[pos..];

        match parsed.expect {
            Expect::Property | Expect::OrderProperty => {
                if parsed.expect == Expect::Property && parsed.clauses.is_empty() && starts_keyword(rest, "OrderBy") {
                    pos += "OrderBy".len();
                    parsed.ordered = true;
                    parsed.expect = Expect::OrderProperty;
                    continue;
                }

                let Some((path, len, unverified)) = path(rest, entity, props, 0) else { break };
                pos += len;
                parsed.unverified = unverified;

                if parsed.expect == Expect::Property {
                    parsed.clauses.push(Clause { path, operator: String::new() });
                    parsed.expect = Expect::Continuation;
                } else {
                    parsed.expect = Expect::Direction;
                }
            }
            Expect::Continuation => {
                if let Some(op) = longest_keyword(rest, &OPERATORS) {
                    pos += op.len();
                    if let Some(clause) = parsed.clauses.last_mut() {
                        clause.operator.push_str(op);
                    }
                } else if let Some(k) = longest_keyword(rest, &["And", "Or"]) {
                    pos += k.len();
                    parsed.expect = Expect::Property;
                } else if let Some(k) = longest_keyword(rest, &["AllIgnoreCase", "AllIgnoringCase"]) {
                    pos += k.len();
                } else if starts_keyword(rest, "OrderBy") {
                    pos += "OrderBy".len();
                    parsed.ordered = true;
                    parsed.expect = Expect::OrderProperty;
                } else {
                    break;
                }
            }
            Expect::Direction => {
                let Some(k) = longest_keyword(rest, &["Asc", "Desc"]) else {
                    parsed.expect = Expect::OrderProperty;
                    continue;
                };
                pos += k.len();
                parsed.expect = Expect::OrderProperty;
            }
        }
    }

    parsed.consumed = if parsed.unverified { predicate.len() } else { pos };
    parsed
}

/// Kotlin and Java names of the same basic types.
pub fn convert_type(ty: &str, kotlin: bool) -> String {
    let pairs = [
        ("Integer", "Int"),
        ("int", "Int"),
        ("long", "Long"),
        ("short", "Short"),
        ("byte", "Byte"),
        ("boolean", "Boolean"),
        ("double", "Double"),
        ("float", "Float"),
        ("char", "Char"),
        ("Character", "Char"),
        ("byte[]", "ByteArray"),
        ("Object", "Any"),
    ];

    if kotlin {
        return pairs.iter().find(|(java, _)| *java == ty).map_or(ty, |(_, k)| k).to_owned();
    }

    let java = [("Int", "Integer"), ("Char", "Character"), ("ByteArray", "byte[]"), ("Any", "Object")];
    java.iter().find(|(k, _)| *k == ty).map_or(ty, |(_, j)| j).to_owned()
}

/// A Java type usable as a type argument.
pub fn boxed(ty: &str) -> String {
    let pairs = [
        ("int", "Integer"),
        ("long", "Long"),
        ("short", "Short"),
        ("byte", "Byte"),
        ("boolean", "Boolean"),
        ("double", "Double"),
        ("float", "Float"),
        ("char", "Character"),
    ];
    pairs.iter().find(|(p, _)| *p == ty).map_or(ty, |(_, b)| b).to_owned()
}

/// The parameter list and return type of a parsed derived query, and the classes they need imported.
fn signature(name: &str, verb: &str, parsed: &Parsed, entity: &str, kotlin: bool) -> (String, Option<String>, Vec<String>) {
    let mut imports = Vec::new();
    let mut params = Vec::new();
    let mut used: Vec<String> = Vec::new();

    for clause in &parsed.clauses {
        let Some(last) = clause.path.last() else { continue };
        let operator = clause.operator.trim_end_matches("IgnoreCase").trim_end_matches("IgnoringCase");
        let base = decapitalize(&clause.path.iter().map(|p| capitalize(&p.name)).collect::<String>());
        let ty = convert_type(&last.ty, kotlin);
        imports.extend(last.fqcn.clone());

        let (names, ty): (Vec<String>, String) = match operator {
            op if NO_ARGUMENT.contains(&op) => continue,
            "Between" | "IsBetween" => (vec![format!("{base}Start"), format!("{base}End")], ty),
            "In" | "IsIn" | "NotIn" | "IsNotIn" => {
                if !kotlin {
                    imports.push("java.util.Collection".into());
                }
                (vec![format!("{base}s")], format!("Collection<{}>", if kotlin { ty } else { boxed(&ty) }))
            }
            _ => (vec![base], ty),
        };

        for mut param in names {
            if used.contains(&param) {
                param = format!("{param}{}", used.len() + 1);
            }
            used.push(param.clone());
            params.push(if kotlin { format!("{param}: {ty}") } else { format!("{ty} {param}") });
        }
    }

    let subject = &name[verb.len()..name.find("By").unwrap_or(verb.len())];
    let single = subject.contains("First") && !subject.chars().any(|c| c.is_ascii_digit() && c != '1')
        || subject.starts_with("Top1") && !subject[4..].starts_with(|c: char| c.is_ascii_digit());

    let returns = match verb {
        "count" => Some(if kotlin { "Long" } else { "long" }.to_owned()),
        "exists" => Some(if kotlin { "Boolean" } else { "boolean" }.to_owned()),
        "delete" | "remove" => (!kotlin).then(|| "void".to_owned()),
        "stream" => {
            imports.push("java.util.stream.Stream".into());
            Some(format!("Stream<{entity}>"))
        }
        _ if single && kotlin => Some(format!("{entity}?")),
        _ if single => {
            imports.push("java.util.Optional".into());
            Some(format!("Optional<{entity}>"))
        }
        _ => {
            if !kotlin {
                imports.push("java.util.List".into());
            }
            Some(format!("List<{entity}>"))
        }
    };

    (params.join(", "), returns, imports)
}

#[derive(Debug, PartialEq)]
pub enum ItemKind {
    Keyword,
    Property,
    Operator,
    Method,
}

#[derive(Debug)]
pub struct Item {
    pub label: String,
    pub kind: ItemKind,
    pub detail: Option<String>,
    /// Replaces the word at the caret.
    pub insert: String,
    pub imports: Option<Edit>,
}

impl Item {
    fn word(label: String, kind: ItemKind, detail: &str) -> Self {
        Self { insert: label.clone(), label, kind, detail: Some(detail.into()), imports: None }
    }
}

fn property_items(base: &str, entity: &str, props: Props, out: &mut Vec<Item>) {
    for prop in props(entity).unwrap_or_default() {
        let label = format!("{base}{}", capitalize(&prop.name));
        out.push(Item::word(label.clone(), ItemKind::Property, &prop.ty));

        let nested = prop.target.as_deref().and_then(props).unwrap_or_default();
        for sub in nested.iter().filter(|s| s.target.is_none()) {
            out.push(Item::word(format!("{label}{}", capitalize(&sub.name)), ItemKind::Property, &sub.ty));
        }
    }
}

/// Completion for the word ending at `at` in a repository.
pub fn complete(text: &str, at: usize, repo: &Repository, props: Props, kotlin: bool) -> Vec<Item> {
    let b = text.as_bytes();
    let start = at - b[..at].iter().rev().take_while(|&&c| is_ident(c)).count();
    let word = &text[start..at];
    let mut items = Vec::new();

    let Some((verb, predicate_start)) = split_subject(word) else {
        for starter in ["findBy", "findAllBy", "findFirstBy", "findDistinctBy", "countBy", "existsBy", "deleteBy", "streamBy"] {
            items.push(Item::word(starter.into(), ItemKind::Keyword, "derived query"));
        }
        if VERBS.iter().any(|v| starts_keyword(word, v)) {
            items.push(Item::word(format!("{word}By"), ItemKind::Keyword, "derived query"));
        }
        return items;
    };

    let predicate = &word[predicate_start..];
    let parsed = parse(predicate, &repo.entity, props);
    if parsed.unverified {
        return items;
    }

    let base = &word[..predicate_start + parsed.consumed];

    match parsed.expect {
        Expect::Property | Expect::OrderProperty => property_items(base, &repo.entity, props, &mut items),
        Expect::Continuation => {
            let last = parsed.clauses.last();
            if last.is_some_and(|c| c.operator.is_empty()) {
                for op in SUGGESTED {
                    items.push(Item::word(format!("{base}{op}"), ItemKind::Operator, "condition"));
                }
            }
            for keyword in ["And", "Or", "OrderBy"] {
                items.push(Item::word(format!("{base}{keyword}"), ItemKind::Keyword, "derived query"));
            }
            if let Some(target) = last.filter(|c| c.operator.is_empty()).and_then(|c| c.path.last()?.target.as_deref()) {
                let props_of_target = props(target).unwrap_or_default();
                for sub in props_of_target.iter().filter(|s| s.target.is_none()) {
                    items.push(Item::word(format!("{base}{}", capitalize(&sub.name)), ItemKind::Property, &sub.ty));
                }
            }
        }
        Expect::Direction => {
            for direction in ["Asc", "Desc"] {
                items.push(Item::word(format!("{base}{direction}"), ItemKind::Keyword, "sort order"));
            }
        }
    }

    if parsed.complete(predicate) && !text[at..].trim_start().starts_with('(') {
        items.push(method_item(text, start, word, verb, &parsed, repo, kotlin));
    }

    items
}

/// The whole declaration of the derived query `word`.
fn method_item(text: &str, start: usize, word: &str, verb: &str, parsed: &Parsed, repo: &Repository, kotlin: bool) -> Item {
    let (params, returns, imports) = signature(word, verb, parsed, &repo.entity, kotlin);
    let line_start = text[..start].rfind('\n').map_or(0, |p| p + 1);
    let before = text[line_start..start].trim();
    let fresh_line = before.is_empty();

    let insert = match (kotlin, &returns) {
        (true, Some(r)) => format!("{}{word}({params}): {r}", if before.ends_with("fun") { "" } else { "fun " }),
        (true, None) => format!("{}{word}({params})", if before.ends_with("fun") { "" } else { "fun " }),
        (false, Some(r)) if fresh_line => format!("{r} {word}({params});"),
        (false, _) => format!("{word}({params});"),
    };

    let imports: Vec<&str> = imports.iter().map(String::as_str).collect();
    Item {
        label: format!("{word}({params})"),
        kind: ItemKind::Method,
        detail: returns.or_else(|| Some("Unit".into())),
        insert,
        imports: import_edit(text, &imports, kotlin),
    }
}

/// Why the derived query `method` doesn't parse, if it doesn't.
pub fn problem(method: &str, entity: &str, props: Props) -> Option<String> {
    let (_, predicate_start) = split_subject(method)?;
    let predicate = &method[predicate_start..];
    let parsed = parse(predicate, entity, props);

    if parsed.unverified || parsed.complete(predicate) {
        return None;
    }

    let rest = &predicate[parsed.consumed..];
    if rest.is_empty() {
        return Some(format!("'{method}' ends where a property of {entity} is expected."));
    }

    let unknown: String =
        rest.char_indices().take_while(|&(i, c)| i == 0 || !c.is_uppercase()).map(|(_, c)| c).collect();
    Some(format!("{entity} has no property '{}' (at '{rest}').", decapitalize(&unknown)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prop(name: &str, ty: &str, target: Option<&str>) -> Prop {
        Prop { name: name.into(), ty: ty.into(), fqcn: None, target: target.map(Into::into) }
    }

    fn props(class: &str) -> Option<Vec<Prop>> {
        match class {
            "Order" => Some(vec![
                prop("id", "Long", None),
                prop("number", "String", None),
                prop("total", "BigDecimal", None),
                prop("customer", "Customer", Some("Customer")),
                prop("customerNote", "String", None),
                prop("supplier", "Supplier", Some("Supplier")),
            ]),
            "Customer" => Some(vec![prop("name", "String", None), prop("age", "int", None)]),
            _ => None,
        }
    }

    const REPO: &str = "package shop;

import org.springframework.data.jpa.repository.JpaRepository;

public interface OrderRepository extends JpaRepository<Order, Long> {
    List<Order> findByCustomerName(String name);

    @Query(\"select o from Order o\")
    List<Order> everything();

    default void helper() {}
}
";

    #[test]
    fn reads_a_repository() {
        let repo = parse_repository(REPO, false).unwrap();
        assert_eq!((repo.entity.as_str(), repo.id_type.as_str()), ("Order", "Long"));
        assert_eq!(&REPO[repo.id_start..repo.id_end], "Long");
        let methods: Vec<(&str, bool)> = repo.methods.iter().map(|m| (m.name.as_str(), m.declared_query)).collect();
        assert_eq!(methods, [("findByCustomerName", false), ("everything", true)]);

        let kotlin = parse_repository("interface R : CrudRepository<shop.Customer, Long?> {\n  fun countByName(n: String): Long\n}", true).unwrap();
        assert_eq!((kotlin.entity.as_str(), kotlin.id_type.as_str(), kotlin.methods[0].name.as_str()), ("Customer", "Long", "countByName"));
        assert!(parse_repository("interface Foo { void x(); }", false).is_none());
    }

    #[test]
    fn parses_paths_operators_and_ordering() {
        let p = parse("CustomerNameContainingIgnoreCaseAndTotalBetweenOrderByNumberDesc", "Order", &props);
        assert!(p.complete("CustomerNameContainingIgnoreCaseAndTotalBetweenOrderByNumberDesc"));
        assert_eq!(p.clauses[0].path.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["customer", "name"]);
        assert_eq!(p.clauses[0].operator, "ContainingIgnoreCase");
        assert_eq!(p.clauses[1].operator, "Between");

        let longest = parse("CustomerNote", "Order", &props);
        assert_eq!(longest.clauses[0].path[0].name, "customerNote");
        assert!(parse("Customer_Age", "Order", &props).complete("Customer_Age"));
        assert!(parse("SupplierCity", "Order", &props).unverified, "Supplier isn't known");
    }

    #[test]
    fn reports_unknown_properties() {
        assert_eq!(problem("findByNumberAndColour", "Order", &props).unwrap(), "Order has no property 'colour' (at 'Colour').");
        assert!(problem("findByNumberAnd", "Order", &props).unwrap().contains("ends where a property"));
        assert!(problem("findByCustomerAgeGreaterThan", "Order", &props).is_none());
        assert!(problem("findAllByOrderByTotalDesc", "Order", &props).is_none());
        assert!(problem("findAll", "Order", &props).is_none());
        assert!(problem("findBySupplierCityName", "Order", &props).is_none());
    }

    fn labels(text: &str, kotlin: bool) -> Vec<String> {
        let repo = parse_repository(text, kotlin).unwrap();
        let at = text.find('|').unwrap();
        let text = text.replace('|', "");
        complete(&text, at, &repo, &props, kotlin).into_iter().map(|i| i.label).collect()
    }

    #[test]
    fn completes_properties_operators_and_the_whole_method() {
        let text = "interface R extends JpaRepository<Order, Long> {\n    findByCus|\n}";
        let found = labels(text, false);
        assert!(found.contains(&"findByCustomer".to_owned()));
        assert!(found.contains(&"findByCustomerName".to_owned()));

        let text = "interface R extends JpaRepository<Order, Long> {\n    findByCustomerNameAndTotalBetween|\n}";
        let repo = parse_repository(text, false).unwrap();
        let at = text.find('|').unwrap();
        let clean = text.replace('|', "");
        let items = complete(&clean, at, &repo, &props, false);
        let method = items.iter().find(|i| i.kind == ItemKind::Method).unwrap();
        assert_eq!(
            method.insert,
            "List<Order> findByCustomerNameAndTotalBetween(String customerName, BigDecimal totalStart, BigDecimal totalEnd);"
        );
        assert_eq!(method.imports.as_ref().unwrap().text, "import java.util.List;\n\n");

        let kotlin = labels("interface R : JpaRepository<Order, Long> {\n    fun findFirstByNumber|\n}", true);
        assert!(kotlin.contains(&"findFirstByNumber(number: String)".to_owned()));
        assert!(kotlin.contains(&"findFirstByNumberIgnoreCase".to_owned()));
    }

    #[test]
    fn kotlin_signature_of_a_single_result() {
        let text = "interface R : JpaRepository<Order, Long> {\n    fun findFirstByNumber|\n}";
        let repo = parse_repository(text, true).unwrap();
        let at = text.find('|').unwrap();
        let items = complete(&text.replace('|', ""), at, &repo, &props, true);
        let method = items.iter().find(|i| i.kind == ItemKind::Method).unwrap();
        assert_eq!(method.insert, "findFirstByNumber(number: String): Order?");
        assert!(method.imports.is_none());
    }
}
