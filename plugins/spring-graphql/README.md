# Spring for GraphQL Tools

Links Spring for GraphQL controllers to their schema in OpenIDEA. The GraphQL language itself (highlighting, the optional language server) is built into the IDE.

- **Handler → schema**: a gutter arrow on each `@QueryMapping`, `@MutationMapping`, `@SubscriptionMapping`, `@SchemaMapping` and `@BatchMapping` method of a `@Controller` opens its schema field; Go to Definition works on the method name and the annotation.
- **Schema → handler**: in `.graphqls`/`.graphql` files, a gutter arrow on each field served by a controller method, and Go to Definition from the field name.
- **Completion** of type names in `typeName = "…"` and of field names in `field`/`name`/`value = "…"`.
- **Checks**: a warning on a handler whose `Type.field` the schema doesn't declare (only when the schema declares that type, or for a root type when the project has a schema), and on an `@Argument` whose name isn't one of the field's arguments; a hint on `Query`/`Mutation`/`Subscription` fields no controller method serves (they may come from `DataFetcher` beans or Querydsl/Query by Example repositories).

How matching works: every `*.graphqls`/`*.graphql` file outside `node_modules` is read as SDL (`type`, `interface`, `input`, `extend type` and the `schema { query: … }` block; operations, enums, unions and scalars are skipped), files under `resources/graphql/` first. A handler's type is the root type of its annotation, else `typeName`, else the class-level `@SchemaMapping(typeName)`, else the simple type of the source parameter (the first one that isn't an `@Argument`/`@ContextValue`/… or an environment type such as `DataFetchingEnvironment`), or the element type of a `@BatchMapping`'s `List<T>`. Its field is the annotation's `field`/`name`/`value`, else the method name. Text-based and Java only: types match by simple name, as written.

## Not ported

- Endpoints tool window entries and HTTP client request generation for GraphQL.
- Checks of client documents (`HttpGraphQlClient`, `GraphQlTester` documents).
- Kotlin controllers.
- Federation directives (`@key`, `@external`…) are skipped, not understood.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
