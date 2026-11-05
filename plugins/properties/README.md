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
  keys it doesn't find.
- Properties: Reformat File applies the code style settings (delimiter
  spacing, alignment, blank lines).
- Properties Structure tool window: keys grouped by dotted prefix, flat or
  sorted. Double-click a key to go to it.
- Open With › Resource Bundle: a table of keys × locales. Edit keys and this
  file's values, and insert or delete rows. Other locales are read-only.
- Go to Declaration on a key: the same key in the parent locale
  (`messages_fr_CA` → `messages_fr` → `messages`); on a Java/Kotlin string:
  every declaration of that key.
- Key completion in `simplelogger.properties` (slf4j) and in the key argument
  of Java/Kotlin i18n calls (`getString`, `getMessage`, `message`…).
- Properties: Copy Property Key / Copy Property Value copy the key or value
  under the caret (unescaped).
- New › XML Properties File.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
