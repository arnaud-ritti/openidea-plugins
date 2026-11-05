# Spring Modulith Tools

Spring Modulith support for Java projects in OpenIDEA:

- **Module model**: the application package is the package of the first `@SpringBootApplication` or `@Modulithic` class (plus `@Modulithic(additionalPackages = …)`), and each of its direct sub-packages is a module. A module's API is its base package. A sub-package is internal unless its `package-info.java` has `@NamedInterface` (named after the annotation value, else the package's last segment; its sub-packages belong to it). A type annotated `@NamedInterface` is exposed on its own, and `@ApplicationModule(type = ApplicationModule.Type.OPEN)` exposes every type.
- **Dependency checks**: in a module's Java files, a warning on each import of another module's internal type and on each import the module's `allowedDependencies` doesn't list (`b` for the API, `b::iface` for a named interface, `b :: *` for all).
- **`allowedDependencies`** in `package-info.java`: completion of module names and `module::interface`, and a warning for an unknown module or interface.
- **Quick fix** on an internal-type warning, when the target module's `package-info.java` has an `@ApplicationModule` without `type`: make that module open.
- **Modules tool window**: one entry per module (open or closed, display name) with its named interfaces, its allowed dependencies and the modules it imports from. A dependency that breaks the rules gets a warning icon. Activating an entry opens the module's `package-info.java` or its first type.

Matching is text-based and goes by imports: fully qualified names in code, imports from libraries and code under `src/test/` are not checked. Packages are expected to follow the lowercase naming convention (`com.acme.order.Order.Inner` is type `Order` of package `com.acme.order`).

Not ported: module documentation generation (C4/PlantUML), the event publication registry, `@ApplicationModuleTest` support, import cycle detection, `sharedModules`, jMolecules annotations, Kotlin.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
