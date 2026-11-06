//! The members Spring Data's own repository interfaces give a repository.

/// Interface, its parents and the members it declares.
const TABLE: &[(&str, &[&str], &[&str])] = &[
    ("Repository", &[], &[]),
    ("CrudRepository", &["Repository"], &[
        "save(S entity)", "saveAll(Iterable<S> entities)", "findById(ID id)", "existsById(ID id)", "findAll()",
        "findAllById(Iterable<ID> ids)", "count()", "deleteById(ID id)", "delete(T entity)", "deleteAllById(Iterable<ID> ids)",
        "deleteAll(Iterable<T> entities)", "deleteAll()",
    ]),
    ("ListCrudRepository", &["CrudRepository"], &["saveAll(Iterable<S> entities)", "findAll()", "findAllById(Iterable<ID> ids)"]),
    ("PagingAndSortingRepository", &["Repository"], &["findAll(Sort sort)", "findAll(Pageable pageable)"]),
    ("ListPagingAndSortingRepository", &["PagingAndSortingRepository"], &["findAll(Sort sort)"]),
    ("QueryByExampleExecutor", &[], &[
        "findOne(Example<S> example)", "findAll(Example<S> example)", "findAll(Example<S> example, Sort sort)",
        "findAll(Example<S> example, Pageable pageable)", "count(Example<S> example)", "exists(Example<S> example)",
        "findBy(Example<S> example, Function queryFunction)",
    ]),
    ("ListQueryByExampleExecutor", &["QueryByExampleExecutor"], &["findAll(Example<S> example)", "findAll(Example<S> example, Sort sort)"]),
    ("JpaRepository", &["ListCrudRepository", "ListPagingAndSortingRepository", "QueryByExampleExecutor"], &[
        "flush()", "saveAndFlush(S entity)", "saveAllAndFlush(Iterable<S> entities)", "deleteAllInBatch(Iterable<T> entities)",
        "deleteAllByIdInBatch(Iterable<ID> ids)", "deleteAllInBatch()", "getReferenceById(ID id)", "findAll(Example<S> example)",
        "findAll(Example<S> example, Sort sort)",
    ]),
    ("JpaSpecificationExecutor", &[], &[
        "findOne(Specification<T> spec)", "findAll(Specification<T> spec)", "findAll(Specification<T> spec, Pageable pageable)",
        "findAll(Specification<T> spec, Sort sort)", "count(Specification<T> spec)", "exists(Specification<T> spec)",
        "delete(Specification<T> spec)",
    ]),
    ("MongoRepository", &["ListCrudRepository", "ListPagingAndSortingRepository", "ListQueryByExampleExecutor"], &[
        "insert(S entity)", "insert(Iterable<S> entities)",
    ]),
    ("ReactiveCrudRepository", &["Repository"], &[
        "save(S entity)", "saveAll(Iterable<S> entities)", "findById(ID id)", "existsById(ID id)", "findAll()",
        "findAllById(Iterable<ID> ids)", "count()", "deleteById(ID id)", "delete(T entity)", "deleteAll()",
    ]),
    ("ReactiveSortingRepository", &["Repository"], &["findAll(Sort sort)"]),
];

/// `(declaring interface, member)` of every known interface `parents` bring in, parents before children, each once.
pub fn inherited(parents: &[&str]) -> Vec<(&'static str, &'static str)> {
    let mut seen: Vec<&'static str> = Vec::new();
    let mut order: Vec<&'static str> = Vec::new();
    let mut pending: Vec<&str> = parents.to_vec();
    while let Some(name) = pending.pop() {
        let Some((known, up, _)) = TABLE.iter().find(|(n, _, _)| *n == name) else { continue };
        if seen.contains(known) {
            continue;
        }
        seen.push(known);
        order.push(known);
        pending.extend(up.iter().copied());
    }
    order.reverse();
    order
        .into_iter()
        .flat_map(|name| {
            let members = TABLE.iter().find(|(n, _, _)| *n == name).map_or(&[][..], |(_, _, m)| m);
            members.iter().map(move |m| (name, *m))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_members_through_known_parents_only() {
        let all = inherited(&["JpaRepository", "Custom"]);
        assert!(all.contains(&("CrudRepository", "findById(ID id)")));
        assert!(all.contains(&("JpaRepository", "flush()")));
        assert!(all.iter().all(|(owner, _)| *owner != "Custom"));
        assert!(inherited(&["Custom"]).is_empty());
    }
}
