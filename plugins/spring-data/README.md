# Spring Data Tools

Spring Data repository support for Java projects in OpenIDEA:

- **Derived query checks**: repository methods without `@Query`/`@NativeQuery`/`@Procedure` are parsed the way Spring Data parses them (`find|read|get|query|search|stream|count|exists|delete|remove` … `By`, criteria split on `And`/`Or`, operator keywords, `IgnoreCase`/`AllIgnoreCase`, `OrderBy … Asc|Desc`). Each property path is resolved on the domain class (longest camel-case prefix first, `_` as explicit separator, nested types, collections unwrapped): an unknown property is a warning at that part of the name. The criteria must take as many arguments as the method declares (`Between` 2, `IsNull`/`True`/`Empty`… 0, others 1; `Pageable`, `Sort`, `Limit`, `ScrollPosition`, `Class` don't count).
- **Completion** of derived query names: after `findBy`, `countBy`, `findAllByNameAnd`… the entity's properties (and nested ones one level deep), then operator keywords, `And`/`Or`/`OrderBy`, `Asc`/`Desc`.
- **Navigation**: a gutter arrow on the repository opens its domain class (and `<Repository>Impl` when there is one); Go to Definition on a property part of a method name opens the field.
- **`@Query` checks**: a warning on a `:name` parameter that no method parameter binds (by `@Param("name")` or by the parameter name), and on `@Modifying` without `@Query`.

A repository is an interface extending a `…Repository`/`…RepositoryBase` type with type arguments (the first is the domain type) or annotated `@RepositoryDefinition(domainClass = …)`. Properties are the non-static, non-transient fields (not `@Transient`) and record components of the domain class and its superclasses. Matching is text-based, by simple type name among the project's Java sources: when the domain class, a superclass or a traversed type isn't a project class (a library type, Kotlin), nothing is reported.

Not ported: JPQL/HQL/SQL injection and highlighting in `@Query`, a preview of the resolved query, Kotlin repositories, MongoDB and other store specifics (geo `Near`/`Within` arguments aren't counted, JSON queries aren't checked), navigation to repository fragments other than `<Repository>Impl`.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
