# Ideolog

OpenIDEA port of JetBrains' [Ideolog](https://github.com/JetBrains/ideolog) (MIT), from tag
`pycharm/263.7307.101` (`41beb53`), the version on the JetBrains Marketplace.

- Detects the log format from the first 25 lines: IntelliJ IDEA, TeamCity build log, Logcat,
  Loguru, pipe-separated, or your own; other logs are split on the first space, like Ideolog does.
- Colours each event by level (error, warning, info), including its stack trace lines. The time
  column of a coloured event gets its own colour.
- Folds multi-line events (stack traces).
- Lists errors and warnings in the structure view.
- **Ideolog: Jump to Source** (F7): opens the stack frame on the caret line (Java/Kotlin, Python,
  .NET, PHP, Node, Go, Rust…), or else the source line whose string literal best matches the
  event's message.
- **Ideolog: Go to Next Error** (Shift+F7), **Go to Previous / Next Entry**.
- Cmd/Ctrl+click on a stack frame opens its file and line.

Colours come from the theme's syntax colours (keyword, tag, string, number…), which are close to
Ideolog's red, orange and green in the default schemes.

## Custom formats

Settings › Plugins › Ideolog › _Custom log formats and highlighting_: paste the
`<LogParsingPattern …/>` and `<LogHighlightingPattern …/>` elements of an Ideolog settings export,
or a file from Ideolog's [custom log formats](https://github.com/JetBrains/ideolog/wiki/Custom-Log-Formats)
(Laravel, Symfony, Serilog, Xdebug…). Patterns use Java-like regular expressions, lookaround
included. Highlighting colours map to the nearest theme colour; `HIDE` patterns are ignored.
Open logs pick up a change when they're edited or reopened.

## `.log` files

OpenIDEA has a built-in `Log` file type that owns the `log` extension, and a plugin can't take
over a built-in extension. The language claims `*.log` as a file name pattern instead, which the
IDE matches before extensions. If a later IDE closes that gap, map the files under
**Settings › File Types › Log (Ideolog)**. Commands and stack frame links also work in files of
the built-in `Log` type.

## Not ported

No OpenIDEA API for these yet:

- Hiding, whitelisting or highlighting values and columns, hiding lines above/below or containing
  a substring, time-gap highlighting: the editor has no API to fold, hide or re-colour on demand.
- Extend selection: plugins can't set the selection.
- Error stripe heat map: no editor decoration API.
- Large file editor, terminal and TextMate integrations.

Stack frame links and syntax come from one component through a hand-expanded `register_plugin!`
(SDK 0.1.37 has no `syntax, providers` arm), see `src/lib.rs`.

Navigation needs the log inside the open project (the IDE only opens project files for plugins).
Jump to Source scans up to 5,000 source files, as there is no index to query. Only the first 512 KiB
of a log is highlighted, to keep within the IDE's 50 ms editor budget.
