# Spring OSGi

Spring Dynamic Modules (Spring OSGi) support for Spring XML files in OpenIDEA.
Port of JetBrains' Spring OSGi plugin (`com.intellij.spring.osgi` 253.28294.218,
Apache-2.0) from [intellij-obsolete-plugins/spring-osgi](https://github.com/JetBrains/intellij-obsolete-plugins/tree/07887c0596764e16977fd4570c2234bfc81dde49/spring-osgi),
by JetBrains s.r.o. and contributors. See `LICENSE` and `NOTICE`.

Works in any Spring XML file that declares `http://www.springframework.org/schema/osgi`
(or `osgi-compendium`) under any prefix or as the default namespace.

- **Inspections** (errors, with the original plugin's messages):
  - elements inconsistency: `interface` attribute with an `<interfaces>` element, `ref` with a nested `<bean>`, `comparator-ref` with a `<comparator>`;
  - service: the `ref` bean (or nested `<bean class>`) of an `<osgi:service>` must implement every exported interface;
  - listener: `bind-method`/`unbind-method` of an `<osgi:listener>` must be public, return `void` and take `(ServiceType, Map|Dictionary)` or `(ServiceReference)`; a listener without both methods must implement `OsgiServiceLifecycleListener`;
  - listener methods (`bind-method`, `unbind-method`, `registration-method`, `unregistration-method`) that don't exist in the listener's class.
- **Unresolved `ref`** (Info, `unresolved-osgi-ref`): a `ref` on `<osgi:service>`, `<osgi:listener>` or `<osgi:registration-listener>`, a `comparator-ref` on `<osgi:list>`/`<osgi:set>` and any `depends-on` naming no bean of the project. Info because beans from libraries or the container are invisible; placeholders are skipped.
- **Go to Definition and completion**: `ref`, `comparator-ref`, `depends-on` → beans (on `<osgi:service ref>` the beans implementing the exported interfaces sort first; XML ids/names/aliases of the project's Spring XML files, Java beans from stereotypes and `@Bean` methods); `interface` and `<osgi:interfaces><value>` → project Java classes (interfaces first); listener methods → the methods of the listener bean's class that fit the expected signature; while typing `<osgix:` the compendium elements `cm-properties`, `managed-properties` and `managed-service-factory` (hand-written list, whether the host triggers completion inside a tag name is unverified).

How it matches: classes by fully qualified name from the project's Java sources, supertypes through each file's imports, parameter types by simple name. A check only fires when the whole class hierarchy is in the project's sources. When the hierarchy reaches into a library (a superclass from a jar, a wildcard import), the check is skipped.

Not ported:
- type checks needing real type resolution: the registration method's first parameter against the service bean class, beans of an `<osgi:reference>` as service implementors; `ref` completion only sorts, it never hides beans;
- the "Interface is expected here" check (disabled upstream too);
- `ReferenceEffectiveTypeProvider` (no Spring model to feed bean types into), the compendium DOM model;
- Kotlin sources.

Built instead of the missing Generate menu: the _Generate OSGi bean_ command in the XML editor context menu (service or reference). The `bundle` bean scope is accepted by the Spring Beans plugin's scope check and completion.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
