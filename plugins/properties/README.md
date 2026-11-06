# Properties

Inspections and editing tools for `.properties` files in OpenIDEA, ported
from JetBrains' [Properties plugin](https://plugins.jetbrains.com/plugin/11594-properties)
(`com.intellij.properties`, Apache-2.0, source in
[intellij-community](https://github.com/JetBrains/intellij-community/tree/idea/263.7307.112/plugins/properties)).

- Problems reported on open and save: duplicate keys, invalid `\u` escapes,
  trailing spaces, a key/value delimiter that doesn't match the setting, and
  keys missing from other locales of the resource bundle. Three dots instead of
  `…` and unsorted files are reported when enabled in the settings.
- Quick fixes (⌥⏎): remove property, unescape, remove trailing spaces, replace
  delimiter, add the missing translation to another locale, remove a property
  from every locale, sort the file or the whole bundle.
- Properties: Find Unused Properties searches the project's other files
  (`.properties` files excluded) for each key of the open file and fades the
  keys it doesn't find. The setting "Mark unused keys when a bundle file opens
  or saves" (off by default) does it live, as hints, over at most 2000 files
  under 256 KiB; keys built at run time look unused.
- Properties: Reformat File applies the code style settings (delimiter
  spacing, alignment, blank lines).
- Properties Structure tool window: keys grouped by dotted prefix, flat or
  sorted. Double-click a key to go to it.
- Open With › Resource Bundle: a table of keys × locales. Edit keys and this
  file's values, and insert or delete rows. Editing another locale's cell
  opens that file at the key (a plugin can't write to a file that isn't the
  focused editor). Show Incomplete Only keeps the keys some locale lacks.
- Go to Declaration on a key: the same key in the parent locale
  (`messages_fr_CA` → `messages_fr` → `messages`) and in every other locale
  of the bundle that declares it; on a Java/Kotlin string:
  every declaration of that key; also on `#{a.b}` in `.html` (Thymeleaf),
  `.xhtml` and JSP, and on `<spring:message code>` / `<fmt:message key>`.
- Problems in code: a literal key in `messageSource.getMessage("a.b", …)`,
  `ResourceBundle.getBundle("messages").getString("a.b")`, `#{a.b}` in `.html`,
  `<spring:message>` and `<fmt:message>` that no `.properties` file declares.
  Reported only when the project has exactly one MessageSource-style bundle
  (`messages`, `ValidationMessages` or anything under `i18n/`); dynamic keys,
  default messages, `{0}` patterns and bundles outside the project stay quiet.
  Turn off with `reportMissingKeysInCode`. The check runs when a file is opened
  or saved, not when a `.properties` file changes.
- Key completion in `simplelogger.properties` (slf4j) and in the key argument
  of Java/Kotlin i18n calls (`getString`, `getMessage`, `message`…).
- Properties: Copy Property Key / Copy Property Value copy the key or value
  under the caret (unescaped).
- New › Properties File and New › XML Properties File. The first is a manifest
  entry only and can't be switched off.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
