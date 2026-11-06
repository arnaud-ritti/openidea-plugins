# Spring Data Tools

Spring Data repository support for Java and Kotlin projects in OpenIDEA:

- **Derived query checks**: repository methods without `@Query`/`@NativeQuery`/`@Procedure` are parsed the way Spring Data parses them (`find|read|get|query|search|stream|count|exists|delete|remove` … `By`, criteria split on `And`/`Or`, operator keywords, `IgnoreCase`/`AllIgnoreCase`, `OrderBy … Asc|Desc`). Each property path is resolved on the domain class (longest camel-case prefix first, `_` as explicit separator, nested types, collections unwrapped): an unknown property is a warning at that part of the name. The criteria must take as many arguments as the method declares (`Between` 2, `IsNull`/`True`/`Empty`… 0, others 1; `Pageable`, `Sort`, `Limit`, `ScrollPosition`, `Class` don't count).
- **Completion** of derived query names: after `findBy`, `countBy`, `findAllByNameAnd`… the entity's properties (and nested ones one level deep), then operator keywords, `And`/`Or`/`OrderBy`, `Asc`/`Desc`.
- **Navigation**: a gutter arrow on the repository opens its domain class (and `<Repository>Impl` when there is one); Go to Definition on a property part of a method name opens the field.
- **`@Query` checks**: a warning on a `:name` parameter that no method parameter binds (by `@Param("name")` or by the parameter name), and on `@Modifying` without `@Query`. In JPQL/HQL queries (not `nativeQuery = true`, not concatenated strings): a warning on an entity name after `from`/`join`/`update` that looks like a typo of a project entity or class (other unknown names stay quiet: they may come from a library or `orm.xml`; `from` inside functions such as `extract(year from d)` is not a declaration), and on `alias.property` paths (`u.address.city`) naming no property of a project entity; library types stay quiet.
- **`@Query` completion**: entity names after `from`/`join`/`update`, and the properties of an identification variable's entity (or of a nested type) after `u.`.
- **Types**: an info (`query-parameter-type`) when a derived query parameter and the property it is compared with are both well-known simple types of different kinds (number, text, boolean, date or time), e.g. a `String` for an `int` property; `In` collections are compared by element, other types stay quiet. A warning (`query-return-type`) when `count...` returns a collection or text, `exists...` a collection, text or number, or `delete...` a `Page`/`Slice`. Setting `springData.checkQueryTypes`.
- **MongoDB**: in a `@Query` of a repository file that imports `org.springframework.data.mongodb`, a warning (`mongo-unknown-field`) on a top-level key whose first path segment is no property of the domain class (nor `id`, `_id`, a `$` operator or a `@Field`/`@Column`/`@JsonProperty` name). Nested documents are not followed; a domain class with a library superclass is not checked.
- **JDBC aggregates**: a warning (`jdbc-shared-entity`) on a property of an aggregate root (the domain type of a repository) whose entity (a class with an id) is also referenced by another root; Spring Data JDBC aggregates must not share entities. `@Embedded` properties and entities that are roots themselves are excluded. Setting `springData.checkJdbcMapping`.
- **Derived query tooltip**: a gutter marker on each derived query method that reads the name in words (`find User where first name and last name, order by age desc`).
- **Show Repository Inherited Members** (command and editor context menu): lists the members a repository inherits from Spring Data's own interfaces (`CrudRepository`, `ListCrudRepository`, `PagingAndSortingRepository`, `JpaRepository`, `MongoRepository`, `QueryByExampleExecutor`, `JpaSpecificationExecutor`, reactive ones); custom parent interfaces are not expanded.
- **File template**: Spring Data Repository (Java and Kotlin) in the New File menu.
- **Kotlin**: repositories and entities in `.kt` files are read like Java ones.

A repository is an interface extending a `…Repository`/`…RepositoryBase` type with type arguments (the first is the domain type) or annotated `@RepositoryDefinition(domainClass = …)`. Properties are the non-static, non-transient fields (not `@Transient`) and record components of the domain class and its superclasses. Matching is text-based, by simple type name among the project's Java and Kotlin sources: when the domain class, a superclass or a traversed type isn't a project class (a library type), nothing is reported.

Not ported: language injection and highlighting in `@Query`, a preview of the resolved query, other store specifics (geo `Near`/`Within` arguments aren't counted, nested MongoDB documents aren't followed), navigation to repository fragments other than `<Repository>Impl`.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
