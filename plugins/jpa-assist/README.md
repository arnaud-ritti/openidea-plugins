# JPA Assist

JPA and Spring Data support for Java and Kotlin in OpenIDEA. The plugin reads
the source text only: it ignores comments and strings, and finds classes by
simple name (next to the file first, then anywhere in the project).

## Checks

Entities and repositories are checked when they open and when they are saved.

| Check | Severity | Quick fix |
|---|---|---|
| `@Entity` without `@Id` / `@EmbeddedId` (and no superclass) | error | |
| `final` entity class (Java) | warning | Remove final |
| Kotlin `data class` entity | warning | |
| Lombok `@Data` on an entity | warning | Replace with `@Getter` and `@Setter` |
| Lombok `@EqualsAndHashCode` on an entity | warning | |
| `FetchType.EAGER` on any association | warning | Fetch lazily |
| `@ManyToOne` / `@OneToOne` relying on the eager default | info | Fetch lazily (adds the `FetchType` import) |
| Derived query naming a property the entity doesn't have | error | |
| Repository id type differing from the entity's `@Id` | error | |

Inverse `@OneToOne(mappedBy = …)` sides aren't flagged, because `LAZY` has no
effect there without bytecode enhancement. Methods with `@Query` aren't parsed.

## Repositories

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

- On an entity's class line, ⌥⏎ offers **Generate equals() and hashCode()**
  (comparing the id, stable for Hibernate proxies) and **Generate
  toString()** (id and basic attributes only, never a lazy association).
- **JPA: Copy DDL of Current Entity / of All Entities**: `CREATE TABLE`,
  sequences, join tables and foreign keys, in PostgreSQL or MySQL (setting),
  with Spring Boot's naming (snake case), ready to paste into a Flyway migration.
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
- Plugins can't create files, so generated code goes to the clipboard.

```sh
cargo test                       # logic, against the SDK's mock host
openidea-plugin build            # → dist/plugin.wasm, validated like an install
openidea-plugin link             # load this folder in the IDE (Settings › Plugins)
```
