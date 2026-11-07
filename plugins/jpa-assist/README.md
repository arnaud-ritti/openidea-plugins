# JPA Assist

JPA and Spring Data support for Java and Kotlin in OpenIDEA. The plugin reads
the source text only: it ignores comments and strings, and finds classes by
simple name (next to the file first, then anywhere in the project).

## Checks

Entities and repositories are checked when they open and when they are saved.

| Check | Severity | Quick fix |
|---|---|---|
| `@Entity` without `@Id` / `@EmbeddedId` / `@IdClass`, own or inherited (`entity-missing-id`; quiet when a superclass is a library class or not in the project) | error | |
| `@Temporal`, `@Enumerated`, `@Version` on a type JPA doesn't allow (`attribute-type`) | warning | |
| Lifecycle callback with the wrong parameters or return type (`entity-listener-signature`) | error | |
| `final` entity class (Java) | warning | Remove final |
| Kotlin `data class` entity (quiet when `equals`, `hashCode` and `toString` are all overridden) | warning | |
| Kotlin entity with a `val` attribute (`jpa-val-attribute`) or a read-only collection on an association (`jpa-immutable-collection`); `@Version val` still warns | warning | |
| Entity that is an inner class, an interface or an enum, or is also `@Embeddable` or `@MappedSuperclass` (`jpa-entity-nested`, `jpa-entity-not-a-class`, `jpa-entity-multiple-roles`) | warning | |
| `@EntityListeners` class with no lifecycle callback (`jpa-listener-without-callbacks`) | warning | |
| Lombok `@Data` on an entity | warning | Replace with `@Getter` and `@Setter` |
| Lombok `@EqualsAndHashCode` on an entity | warning | |
| `FetchType.EAGER` on any association | warning | Fetch lazily |
| `@ManyToOne` / `@OneToOne` relying on the eager default | info | Fetch lazily (adds the `FetchType` import) |
| Attribute with two associations, an association on a basic or collection type, `@Id` with `@EmbeddedId`, `@Id` on a field with mapping annotations on getters or the reverse (`jpa-attribute-multiple-relations`, `jpa-relationship-type`, `jpa-multiple-id-kinds`, `jpa-mixed-access`; quiet with `@Access`) | warning | |
| Entity with property access (`@Id` on a getter, no mapping annotation on fields) whose annotated getter has no setter (`jpa-missing-setter`) | info | |
| Association whose target class is in the project but is an `@Embeddable`, a `@MappedSuperclass` or has no `@Entity` (`jpa-relationship-target`; quiet with `orm.xml`, for classes found twice or outside the project) | info | |
| `@EntityGraph("x")`, `@EntityGraph(value = "x")` or `getEntityGraph("x")` naming no `@NamedEntityGraph`; `@EntityGraph(attributePaths = …)` whose first segment is not a property of the repository entity (`jpa-unknown-entity-graph`, `jpa-unknown-entity-graph-path`; quiet with `orm.xml`, a non-literal graph name or a library superclass; setting `jpaAssist.checkEntityGraphs`) | warning | |
| Listener class with constructors but none public without arguments (`jpa-listener-constructor`; Spring-managed listeners are exempt) | info | |
| Kotlin entity with no no-arg constructor while the build names no `kotlin-jpa` plugin (`jpa-kotlin-plugin-missing`; read from `build.gradle(.kts)` and `pom.xml` up to the project root, quiet for version catalogs, convention plugins and `noArg`) | info | Add the plugin to the Gradle build (copies the Kotlin version) |
| Redundant `constructor()` when the build applies `kotlin-jpa`; redundant `open` when `allOpen` names `jakarta.persistence.Entity` | info | Remove it |
| Unknown attribute in `@AttributeOverride`, `@MapsId`, `@MapKey` or a top-level `@NamedAttributeNode` (`jpa-unknown-*`; quiet when the class is not in the project, its name exists twice or a superclass is a library class) | warning | |
| `:name` in a `@Query` string that is not a parameter of the method or its `@Param`; `@Query(name = …)` or `createNamedQuery(…)` naming no `@NamedQuery` (quiet with `orm.xml` or `jpa-named-queries.properties`) | warning | |
| `persistence.xml` / mapping file: invalid `transaction-type`, `shared-cache-mode`, `validation-mode`, `exclude-unlisted-classes`, `access`, missing or duplicate unit name, bad `<provider>` or `<class>` | warning | |
| `<class>` or mapping `class` that is not in the project's sources, listed twice, or without a JPA annotation (`jpa-xml-class-*`) | info | |
| Derived query naming a property the entity doesn't have | error | |
| Repository id type differing from the entity's `@Id` | error | |

The Kotlin checks look at the first class in a file and judge collection types by simple name, so a
user-defined `List` is flagged. Settings: `jpaAssist.checkKotlinEntities`, `jpaAssist.checkClassSignature`
and `jpaAssist.checkListenerUsage`. A listener is found by its file name and only when one file
matches; callbacks it inherits are not followed.

Inverse `@OneToOne(mappedBy = …)` sides aren't flagged, because `LAZY` has no
effect there without bytecode enhancement. Methods with `@Query` aren't parsed.

## Repositories

In a `persistence.xml`, typing inside `<class>` completes class names of the project's sources, and typing
inside `<property name="…">` completes the standard `jakarta.persistence.*` (or `javax.persistence.*`, when the
file uses that namespace) and common Hibernate property names; `value="…"` completes the values of
`hibernate.hbm2ddl.auto`, schema generation, shared cache and validation mode. Unverified in the IDE: how the host
replaces a name containing dots.

In an interface extending `JpaRepository<Entity, Id>` (or any `…Repository`),
completion builds derived query names: `findBy`, `countBy`, `existsBy`…, the
entity's properties (and one level into its associations), conditions
(`Containing`, `Between`, `IgnoreCase`…), `And`/`Or`, `OrderBy` and
`Asc`/`Desc`. Once the name is complete, one item writes the whole
declaration with parameters and return type (`List`, `Optional` for
`First`/`Top1`, `long` for `count`, …) and adds the imports it needs.

## Navigation

A gutter arrow on an `@Entity` class opens the Spring Data repository that
declares it as domain type (the tooltip lists all of them; the arrow opens the
first). Repository files are found by the `*Repository.java` / `.kt` name. The
Only repositories naming the entity directly as the `…Repository<Entity, Id>` argument are found (a `UserRepo extends BaseRepo<User>` whose `BaseRepo` extends `JpaRepository` is not resolved), and repositories not named `*Repository` are missed. The repositories are read once and updated when a repository file is opened or saved. The reverse arrow, from a repository to its entity, comes from Spring Data.

## Generators

- On a `@ManyToOne`, `@OneToOne` or `@ManyToMany` without `mappedBy`, **Add inverse attribute** adds the mirrored
  attribute (`@OneToMany(mappedBy = …)`, plural name) with its imports at the end of the target entity's file. The
  action writes to the other file only; it is not offered when the target already mirrors the association.

- On an entity's class line, ⌥⏎ offers **Generate equals() and hashCode()**
  (comparing the id, stable for Hibernate proxies) and **Generate
  toString()** (id and basic attributes only, never a lazy association).
- **JPA: Copy DDL of Current Entity / of All Entities**: `CREATE TABLE`,
  sequences, join tables and foreign keys, in PostgreSQL or MySQL (setting),
  with Spring Boot's naming (snake case), ready to paste into a Flyway migration.
- On an entity's class line, seven **Add @PrePersist callback** style actions (one per
  lifecycle annotation) add the method and its import. An action is skipped when the class already has
  a method of that name.
- **JPA: Copy DTO of Current Entity**: a Java record or Kotlin data class
  with the basic attributes and the ids of to-one associations.
- **JPA: Copy Entity from CREATE TABLE at Caret** (in any SQL file): the
  entity for that table, with `@Id`, `@Column`, `@ManyToOne` for foreign
  keys and, in Java, getters and setters. A setting picks Java or Kotlin.

The context-menu entries show only where they apply: Copy DDL and Copy DTO in
Java and Kotlin files, Copy Entity in SQL files.

Without the `clipboard` permission, a notification asks you to allow it in
Settings › Plugins.

## JPA Structure

A tool window listing the project's entities and their attributes (id,
associations, columns). Double-click an item to open its declaration. Refresh
rereads the project.

## Templates

New › JPA Entity / Spring Data Repository (Java and Kotlin); `$END$` leaves
the caret on the repository's entity type.

## Limits

- Inheritance strategies aren't modelled: a superclass's attributes are
  added to the subclass's table.
- Classes sharing a simple name in different packages are told apart only by
  being next to the file that uses them.
- Generated code goes to the clipboard. **JPA: New Entity** also writes `<Name>.java` or `.kt` next to the focused
  file (same package, inside the project, never over an existing file) when `process:tee` is allowed, and fills the
  focused file instead when it is empty. Other generators only use the clipboard.
- Only classes are read as entities: one on an interface or enum is missing from JPA Structure
  and the generators.

```sh
cargo test                       # logic, against the SDK's mock host
openidea-plugin build            # → dist/plugin.wasm, validated like an install
openidea-plugin link             # load this folder in the IDE (Settings › Plugins)
```
