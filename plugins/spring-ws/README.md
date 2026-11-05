# Spring Web Services Tools

Spring Web Services support for Java projects in OpenIDEA:

- **Endpoint ↔ schema**: on an `@Endpoint` class, a `@PayloadRoot(namespace = …, localPart = "x")` method gets a gutter arrow to the top-level element `x` of a schema with that target namespace; Go to Definition on the `localPart` literal does the same. In the schema, the element gets an arrow back to the method that handles it.
- **Completion** of element names in `localPart = "…"`, from the schemas of the method's namespace.
- **Unknown element**: a warning on `localPart` when a schema of that namespace exists but has no such top-level element.
- **Classpath resources**: Go to Definition on a `new ClassPathResource("…")` path opens `src/main/resources/<path>` (or `src/test/resources`), and a `.xsd`/`.wsdl` path not found there is a warning.
- **WSDL publication**: a `DefaultWsdl11Definition` bean shows `Published as /<bean name>.wsdl` in the gutter (under the `MessageDispatcherServlet` mapping).

Schemas are the `xs:schema` elements (any prefix) of the project's `.xsd` files and those inside `.wsdl` files. The namespace may be a string literal or a string constant of the same file (`NAMESPACE_URI`); otherwise elements of every schema match. Matching is text-based. Schemas from libraries are not read, and `@SoapAction`/`@Action` methods are ignored because they name no schema element.

Not ported: Spring OXM marshaller configuration checks, Apache CXF, WSDL generation, the `<sws:…>` XML namespace configuration, `@PayloadRoots`, Kotlin.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
