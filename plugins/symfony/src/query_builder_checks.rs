//! Checks of `QueryBuilder` calls, written from the Doctrine ORM and DBAL
//! documentation and upgrade notes. A call is checked only in a file that
//! imports a Doctrine query builder or calls `createQueryBuilder(`, and only
//! when its receiver is such a call chain or a conventionally named variable.

use crate::checks::Finding;
use crate::php_checks::{arguments, trimmed};
use crate::php_text::{imports, mask, skip_whitespace};

const ORM_BUILDER: &str = "Doctrine\\ORM\\QueryBuilder";
const DBAL_BUILDERS: [&str; 2] = ["Doctrine\\DBAL\\Query\\QueryBuilder", "Doctrine\\DBAL\\Connection"];
const RECEIVERS: [&str; 6] = ["$qb", "$queryBuilder", "$builder", "$this->qb", "$this->queryBuilder", "$this->builder"];
const ADDER_PARTS: [&str; 8] = ["select", "from", "join", "set", "where", "groupBy", "having", "orderBy"];
const ARRAY_ARGUMENT_METHODS: [&str; 5] = ["select", "addSelect", "groupBy", "addGroupBy", "orderBy"];
const DBAL_REMOVED_PARTS: [&str; 4] = ["getQueryPart", "getQueryParts", "resetQueryPart", "resetQueryParts"];
const DQL_TIME_NAMES: [&str; 4] = ["CURRENT_DATE", "CURRENT_TIME", "CURRENT_TIMESTAMP", "NOW()"];

fn statement_start(masked: &str, at: usize) -> usize {
    masked[..at].rfind([';', '{', '}']).map_or(0, |found| found + 1)
}

fn on_builder(masked: &str, at: usize) -> bool {
    let statement = masked[statement_start(masked, at)..at].trim_start();
    let statement = statement.strip_prefix("return").map_or(statement, str::trim_start);
    statement.contains("createQueryBuilder(") || RECEIVERS.iter().any(|receiver| statement.starts_with(receiver) && !statement[receiver.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_'))
}

pub fn check(text: &str) -> Vec<Finding> {
    let masked = mask(text);
    let imports = imports(text);
    let orm = imports.iter().any(|import| import.fqcn == ORM_BUILDER) || masked.contains("createQueryBuilder(");
    let dbal = imports.iter().any(|import| DBAL_BUILDERS.contains(&import.fqcn.as_str()));
    if !orm && !dbal {
        return Vec::new();
    }

    let mut found = Vec::new();
    for (at, _) in masked.match_indices("->") {
        let name = crate::php_text::read_word(&masked, at + 2);
        let open = skip_whitespace(&masked, name.end);
        if name.is_empty() || !masked[open..].starts_with('(') || !on_builder(&masked, at) {
            continue;
        }

        let method = &masked[name.clone()];
        let arguments = arguments(&masked, open);
        let literal = |position: usize| arguments.get(position).map(|range| trimmed(&masked, range.clone())).filter(|range| text[range.clone()].starts_with(['\'', '"']));
        let literal_text = |range: &std::ops::Range<usize>| text[range.start + 1..range.end.saturating_sub(1).max(range.start + 1)].to_owned();

        if matches!(method, "orderBy" | "addOrderBy") {
            if let Some(direction) = literal(1).filter(|range| !matches!(literal_text(range).to_ascii_uppercase().as_str(), "ASC" | "DESC")) {
                found.push(Finding::warn(direction, "query-builder-sort-direction", "The sort direction must be ASC or DESC.".into()));
            }
        }

        match method {
            "add" => {
                let Some(part) = literal(0).filter(|range| ADDER_PARTS.contains(&literal_text(range).as_str())) else { continue };
                match dbal {
                    true => found.push(Finding::warn(name, "query-builder-add", "QueryBuilder::add() is deprecated since DBAL 3.1: call the dedicated method such as where() or orderBy().".into())),
                    false => found.push(Finding::hint(part, "query-builder-add", "add('part', ...) is the low-level form: call the dedicated method such as where() or orderBy().".into())),
                }
            }
            m if dbal && DBAL_REMOVED_PARTS.contains(&m) => {
                found.push(Finding::warn(name, "query-builder-parts", format!("QueryBuilder::{m}() is deprecated since DBAL 3.1: use the dedicated getters and reset methods.")));
            }
            "execute" if dbal && arguments.is_empty() => {
                found.push(Finding::warn(name, "query-builder-execute", "QueryBuilder::execute() is deprecated since DBAL 3.1: use executeQuery() or executeStatement().".into()));
            }
            "from" if orm && !dbal && arguments.len() == 1 => {
                found.push(Finding::warn(name, "query-builder-alias", "The DQL query builder needs an alias: from(Entity::class, 'alias').".into()));
            }
            m if ARRAY_ARGUMENT_METHODS.contains(&m) => {
                let Some(first) = arguments.first().map(|range| trimmed(&masked, range.clone())).filter(|range| masked[range.clone()].starts_with('[') || masked[range.clone()].to_ascii_lowercase().starts_with("array(")) else { continue };
                if arguments.len() == 1 && m != "orderBy" {
                    found.push(Finding::hint(first, "query-builder-array", format!("Pass the expressions to {m}() as separate arguments instead of one array.")));
                }
            }
            "setParameters" => {
                let Some(first) = arguments.first().map(|range| trimmed(&masked, range.clone())).filter(|range| masked[range.clone()].starts_with('[')) else { continue };
                found.push(Finding::hint(first, "query-builder-set-parameters", "Bind parameters one by one with setParameter(), or pass an ArrayCollection of Parameter objects.".into()));
            }
            "setParameter" => {
                let Some(value) = literal(1).filter(|range| DQL_TIME_NAMES.contains(&literal_text(range).to_ascii_uppercase().as_str())) else { continue };
                found.push(Finding::warn(value, "query-builder-time-parameter", "A bound parameter is a plain string: write CURRENT_DATE in the DQL, or bind a DateTime.".into()));
            }
            _ => {}
        }

    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(code: &str) -> Vec<(&'static str, String)> {
        let text = format!("<?php\nuse Doctrine\\ORM\\QueryBuilder;\n\nclass R\n{{\n    function f($qb)\n    {{\n{code}\n    }}\n}}\n");
        check(&text).into_iter().map(|finding| (finding.code, text[finding.start..finding.end].to_owned())).collect()
    }

    #[test]
    fn orm_builder_calls() {
        assert_eq!(codes("$qb->add('where', 'x');"), [("query-builder-add", "'where'".to_owned())]);
        assert_eq!(codes("$qb->select('u')->from(User::class);"), [("query-builder-alias", "from".to_owned())]);
        assert!(codes("$qb->select('u')->from(User::class, 'u');").is_empty());
        assert_eq!(codes("$qb->select(['u.a', 'u.b']);"), [("query-builder-array", "['u.a', 'u.b']".to_owned())]);
        assert_eq!(codes("$qb->setParameters(['a' => 1]);"), [("query-builder-set-parameters", "['a' => 1]".to_owned())]);
        assert_eq!(codes("$qb->orderBy('u.a', 'ascending');"), [("query-builder-sort-direction", "'ascending'".to_owned())]);
        assert!(codes("$qb->orderBy('u.a', 'desc')->addOrderBy('u.b', 'ASC');").is_empty());
        assert_eq!(codes("$qb->setParameter('d', 'CURRENT_DATE');"), [("query-builder-time-parameter", "'CURRENT_DATE'".to_owned())]);
    }

    #[test]
    fn other_receivers_and_files_stay_silent() {
        assert!(codes("$list->add('where', 'x'); $this->from(A::class); $repo->orderBy('a', 'x');").is_empty());

        let plain = "<?php\nclass R { function f($qb) { $qb->add('where', 'x'); } }\n";
        assert!(check(plain).is_empty());
    }

    #[test]
    fn dbal_builder_deprecations() {
        let text = "<?php\nuse Doctrine\\DBAL\\Connection;\n\nclass R\n{\n    function f(Connection $c)\n    {\n        $qb = $c->createQueryBuilder();\n        $qb->add('where', 'x');\n        $qb->getQueryPart('where');\n        $c->createQueryBuilder()->select('1')->from('t')->execute();\n    }\n}\n";
        let found: Vec<_> = check(text).into_iter().map(|finding| finding.code).collect();
        assert_eq!(found, ["query-builder-add", "query-builder-parts", "query-builder-execute"]);
    }
}
