# Ideolog

A log viewer. It detects the log format, colours each event by level, folds multi-line events and links stack frames to your source. You can add your own Ideolog parsing and highlighting patterns. MIT; see `LICENSE`.

## Requirements

- OpenIDEA with plugin API 1.14 or later (`engines.openidea = "^1.14"`).
- The log must be inside the open project for navigation, snapshots and marks.
- Optional: Java 17 or later on the `PATH` for **Ideolog: Capture JVM Regex and Date Observations** and **Ideolog: Open Live Log Tail**.
- No accounts or network access.

## Quick start

1. Install the plugin ([how](../../README.md#install-a-plugin)).
2. Open a `.log` file from your project. Events are coloured by level and the status bar shows the detected format (`Log: IntelliJ IDEA`).
3. Press Shift+F7 to jump to the next error.
4. Put the caret on a stack frame and press F7 to open the source line.
5. For full theme colours, switch the file type to **Log (Ideolog)**. See [`.log` files](#log-files).

## Features

### Read logs with colours and folds

- The format is detected from the first 25 lines. Supported: IntelliJ IDEA, TeamCity build log, Logcat, Loguru, pipe-separated, Docker/ISO timestamps, Python `logging`, nginx error log, syslog, Laravel/Monolog (`[date] channel.LEVEL:`), Serilog text, or your own patterns. Yours win ties. Other logs are split on the first space.
- Each event is coloured by level (error, warning, info), stack trace included. The time column gets its own colour.
- Multi-line events such as stack traces fold.
- The structure view lists errors and warnings.
- The status bar names the format (`Log: IntelliJ IDEA` or `Log: plain text`). Click it for a menu: **Go to Next Error**, **Go to Next Warning**, **Go to Next Exception**, **Jump to Source**, **Capture Active Log Snapshot**, **Open Live Log Tail** and **Formats and Highlighting**.
- On a large log, the status bar adds how much is coloured: `(first 1200 lines highlighted)` or `(too large to highlight)`. Over 512 KiB, its tooltip says folding, outline and stack-frame links are off.

### Errors in Run consoles

Ideolog also colours the output of every Run and Debug console: the words `ERROR`, `FATAL`, `SEVERE`, `CRITICAL` (red) and `WARN`, `WARNING` (yellow) in capitals, and the first line of a Java exception (`java.lang.IllegalStateException: …`, `Caused by: …`) or a Python traceback. Only those words are coloured; lines keep the console's own level colours. Turn it off with **Highlight errors, warnings and exceptions in Run consoles**.

### Spot pauses

Set **Mark time gaps (seconds)**. An event that comes at least that many seconds after the previous one gets a coloured time. This works with ISO-like, Logcat, syslog and `HH:MM:SS` times.

### Jump around a log

- **Ideolog: Go to Next Error** (Shift+F7), **Ideolog: Go to Next Warning** and **Ideolog: Go to Next Exception**.
- **Ideolog: Go to Previous Entry** and **Ideolog: Go to Next Entry**.
- **Ideolog: Extend Log Selection** grows the selection from a field to its line, its event and the whole document.

An exception is an event whose text names an exception, or a Python traceback.

### See log levels beside the editor

The **Log Levels** panel sits beside each log editor (needs `editor.read`). It shows:

- The detected format, and a **Formats and Highlighting** link to that tab.
- How many **Errors**, **Warnings**, **Exceptions**, **Info**, **Debug**, **Trace** and **All entries** the log has. A level the format can't write has no row: syslog finds only errors and warnings in the message, and Python `logging` has no trace level.
- Up and down buttons on each row to go to the previous or next one. They move the caret of the focused log, so click in the log first.
- **Show level** and **Filter in snapshot**: capture the focused log into the **Log Snapshot** table showing only that level. The editor can't fold or hide entries for a plugin, so the filter lives in the table.

Levels are read from the format's level column. `NOTICE` and `CONFIG` count as info, `FINE` as debug, `FINER`, `FINEST` and `VERBOSE` as trace. Single letters (`I`, `D`, `T`, `V`) count only in a format's own level column, as in Logcat, never as the first word of a plain log.

On a large log, counts cover the first lines only and show a `+`. The note under the table says how many lines were counted; the buttons still search the whole log.

### Jump to source

- **Ideolog: Jump to Source** (F7) opens the stack frame on the caret line. It supports Java/Kotlin, Python, .NET, PHP, Ruby, Dart (`file://`, `package:`), Swift, Node, Go, Rust and more.
- On a line that isn't a stack frame, it opens the source line whose string literal best matches the event's message.
- Cmd/Ctrl+click a stack frame to open its file and line.

### `.log` files

The IDE's built-in **Log** file type owns `.log` by default. The plugin colours it, folds events and links stack frames.

Some colours don't show in the built-in **Log** editor yet. For full theme colours with your own rules, choose **Log (Ideolog)** as the file type. Commands, stack-frame links, custom patterns and **Mark time gaps (seconds)** work with both types.

### Add your own log formats

Run **Ideolog: Formats and Highlighting** to open the **Formats and Highlighting** tab. It has **Log Formats** and **Highlighting** sections.

1. Click **Add Format** for a new format, or **Add Highlighting** for a colour rule.
2. Fill in the form. For formats: name, event regex, event-start regex, timestamp format (empty means automatic) and 0-based timestamp/level/category columns (`-1` means none).
3. Paste a sample event and click **Preview** to check the captures or colours.
4. Click **Save**. Invalid input stays open with an explanation. **Cancel** discards the draft.

Select a row to **Edit**, **Enable / Disable Selected**, **Move Up**, **Move Down** or **Remove** it. Double-click opens **Edit**.

Text fields commit on blur or Enter. Press Tab before **Preview** or **Save** after typing.

You can also paste `<LogParsingPattern …/>` and `<LogHighlightingPattern …/>` elements into **Custom log formats and highlighting** in Settings. They come from an Ideolog settings export or a [custom log formats file](https://github.com/JetBrains/ideolog/wiki/Custom-Log-Formats) (Laravel, Symfony, Serilog, Xdebug…). Patterns use Java-like regular expressions, lookaround included. Colours map to the nearest theme colour. `HIDE` patterns are ignored.

Open logs pick up a change when you edit or reopen them.

### Highlight text you're looking for

- Select text on one line, right-click and choose **Ideolog: Highlight Selection**. It becomes an escaped highlighting rule.
- **Ideolog: Remove Custom Highlightings** deletes all of them after you confirm. The confirmation gives the count; custom log formats are kept.

### Review a log as a table

1. Focus a log and run **Ideolog: Capture Active Log Snapshot**.
2. The **Log Snapshot** tab opens in the bottom panel. Each event is one row with line, level, timestamp, category and message.
3. Use **Previous** / **Next Events** to page. Activate a row to open that event in the log.
4. Pick a level button (**All**, **Errors**, **Warnings**, **Info**, **Debug**, **Trace**, each with its count) to list only those events.

**Ideolog: Open Captured Log Snapshot** reopens the last snapshot. A snapshot doesn't update; capture again after the log changes.

### Follow a growing log (Java)

1. Open a saved UTF-8 log from your project.
2. Run **Ideolog: Open Live Log Tail** and click **Start**. Confirm **Follow app.log?**; the status line then reads *Following app.log (stops in 5 min)*.
3. New complete lines appear in the panel. Type in the filter to keep matching lines, or check **Hide matches** to hide them.
4. **Follow** keeps the newest line in view. **Stop** ends the session.

Leaving or hiding the panel stops the tail. **Ideolog: Stop Live Log Tail** stops a session left over after a panel reload. A session lasts at most 5 minutes or 1 MiB.

### Test a Java-only pattern (Java)

**Ideolog: Capture JVM Regex and Date Observations** runs your custom format with the real Java regex and `SimpleDateFormat`. Use it when a pattern needs Java features the built-in parser lacks.

1. Focus a project log of up to 64 KiB / 512 lines.
2. Run the command and pick a custom format. Optionally set a BCP47 locale and a Java time zone; empty means the JVM default.
3. Review and approve. Results appear in the **JVM Observations** tab.

The captured timestamps, levels and categories then also colour the built-in **Log** editor until the log or settings change.

### Paint errors and gaps in the built-in Log editor

**Ideolog: Mark Current Log Source** paints errors, warnings and time gaps in the focused built-in **Log** file with native theme decorations. **Ideolog: Clear Log Source Marks** removes them. Marks also disappear when you edit the file; run Mark again to refresh.

## Commands

Find them in Search Everywhere › Actions (⇧⇧) under **Ideolog**; bind keys in Settings › Keymap. **Jump to Source**, the **Go to** commands and **Highlight Selection** show only while a **Log** or **Log (Ideolog)** editor has focus.

| Command | What it does | Shortcut |
|---|---|---|
| **Ideolog: Jump to Source** | Opens the stack frame or matching source line for the event at the caret. | F7 |
| **Ideolog: Go to Next Error** | Moves to the next error event. | Shift+F7 |
| **Ideolog: Go to Next Warning** | Moves to the next warning event. | none |
| **Ideolog: Go to Next Exception** | Moves to the next event that names an exception or a Python traceback. | none |
| **Ideolog: Go to Next Entry** | Moves to the next event. | none |
| **Ideolog: Go to Previous Entry** | Moves to the previous event. | none |
| **Ideolog: Extend Log Selection** | Grows the selection: field, line, event, document. | none |
| **Ideolog: Formats and Highlighting** | Opens the **Formats and Highlighting** tab. | none |
| **Ideolog: Highlight Selection** | Adds the selected text as a highlighting rule. Also in the editor context menu of log files. | none |
| **Ideolog: Remove Custom Highlightings** | Asks for confirmation, then deletes all highlighting rules. Custom log formats are kept. | none |
| **Ideolog: Capture Active Log Snapshot** | Captures the focused log into the **Log Snapshot** tab. Enabled in log editors. | none |
| **Ideolog: Open Captured Log Snapshot** | Reopens the last snapshot. | none |
| **Ideolog: Mark Current Log Source** | Paints errors, warnings and gaps in the built-in **Log** editor. Enabled only there. | none |
| **Ideolog: Clear Log Source Marks** | Removes those marks. | none |
| **Ideolog: Capture JVM Regex and Date Observations** | Runs a custom format with Java and shows the result. Enabled in log editors. | none |
| **Ideolog: Open Live Log Tail** | Opens the live tail panel for the focused log. | none |
| **Ideolog: Stop Live Log Tail** | Stops a running live tail. | none |

## Settings

Settings › Plugins › Ideolog.

| Setting | Key | Default | What it changes |
|---|---|---|---|
| Custom log formats and highlighting | `customPatterns` | empty | Pasted Ideolog `<LogParsingPattern>` and `<LogHighlightingPattern>` XML. |
| Mark time gaps (seconds) | `timeGapSeconds` | `0` (off) | Colours the time of an event after a pause this long. |
| Highlight errors, warnings and exceptions in Run consoles | `consoleHighlighting` | on | Colours level words and exception lines in Run and Debug console output. |

Patterns made in the Formats and Highlighting tab are kept in plugin storage, not in this setting. They work without an open project.

## Permissions

Every permission is optional. Colouring works with none. Allow them in Settings › Plugins › Ideolog.

| Permission | Used for | Without it |
|---|---|---|
| `editor.read` | Reading the log and caret for navigation, snapshots, marks, **Highlight Selection** and the **Log Levels** panel. | Those commands fail and the panel doesn't show. |
| `project.read` | Opening source files, finding the source for **Jump to Source**, snapshots and marks. | Navigation and snapshots fail. |
| `events` | Following the focused editor for the status bar item and refreshing marks and caches. | The status bar item doesn't update. |
| `storage` | Saving patterns from the Formats and Highlighting tab. | You can't save patterns there; the Settings text still works. |
| `process:java` | **Ideolog: Capture JVM Regex and Date Observations** and **Ideolog: Open Live Log Tail**. | Those two fail; everything else works. |

## Troubleshooting

See also the [repository guide](../../README.md#troubleshooting).

| Problem | What to do |
|---|---|
| Custom colours don't show in a `.log` file | The built-in **Log** editor doesn't render every colour yet. Switch the file type to **Log (Ideolog)**. |
| A new pattern doesn't apply to the open log | Edit or reopen the log. |
| "No source line found for this event. Open the project that produced the log." | Open the project whose code wrote the log. |
| F7 or Cmd+click does nothing on a stack frame | The log must be inside the open project. If two files match the frame equally well, nothing opens. |
| "Not a log file." | Focus a file of type **Log** or **Log (Ideolog)**. |
| "Select some text on one line to highlight." | **Highlight Selection** works on one line only. |
| "Pattern not added: …" | Fix the reason shown, then try again. |
| A change in the Formats and Highlighting tab won't save | Allow `storage`. If another window changed the patterns, reopen the view and edit again. |
| "Focus a Log editor, then Capture Active Log Snapshot." | Click into the log editor first. |
| "This log is … KiB; snapshots are limited to 256 KiB and 8192 lines. Use the log editor for larger files." | Read the log in the editor, or capture a smaller excerpt. |
| "Click in this log first: the panel moves the caret of the focused editor." | Click in the log, then use the **Log Levels** buttons again. |
| "Click in this log first: the snapshot captures the focused editor." | Click in the log, then **Filter in snapshot** again. |
| Ideolog commands are missing from Search Everywhere | Navigation and **Highlight Selection** show only while a log editor has focus. |
| The status bar says `(too large to highlight)` or `(first … lines highlighted)` | Colouring stops at the first very long line or after 512 KiB. The **Go to** commands and **Log Levels** buttons still search the whole log. |
| "Snapshot source changed; capture again before following a row." | Run **Ideolog: Capture Active Log Snapshot** again. |
| "Add or import a custom Ideolog LogParsingPattern first. …" | JVM capture needs a custom format. Add one in the Formats and Highlighting tab or Settings. |
| "JVM observations support a nonempty log up to 64KiB/512 lines, each at most 4096 bytes." | Use a smaller log or an excerpt. |
| "Live tail follows saved log files of this project…" | Save the log and make sure it is inside the open project. |
| "Live tail stopped: the log has unsaved edits…" | Save or revert the log, then click **Start** again. |
| "Stop the current live tail before starting another." | Click **Stop**, or run **Ideolog: Stop Live Log Tail**. |
| "Live tail needs the process:java permission…" | Allow `process:java` in Settings › Plugins › Ideolog. Capture JVM Regex and Date Observations shows the same message. |
| A timestamp format is rejected | Use the supported numeric fields listed in Technical details. Quote literal letters. |

## Limitations

- The native editor can't hide lines, whitelist lines, or highlight values and columns. The **Live Log Tail** panel has literal and inverse text filtering only.
- No scrollbar stripe, error-stripe heat map, or custom font and background styles.
- No large-file editor, terminal or TextMate integration.
- Timestamp formats are a numeric subset, not full Java `SimpleDateFormat`.
- The pattern forms have no regex-aware widgets, full colour picker, or background/bold/italic/stripe options.
- Patterns are shared by all windows. Two windows editing at once can overwrite each other.
- Live tail isn't available on Windows.

## Technical details

<details>
<summary>Colours, limits, caching and safety checks</summary>

### Colours and levels

- Colours come from the theme's syntax colours (keyword, tag, string, number…), which approximate red, orange and green in the default schemes. Warnings and marked time gaps use `enum-member` because LSP semantic tokens have no `tag` or `constant` kind.
- Syslog levels come from keywords.
- Debug and trace keep the editor colour, and the structure view lists only errors and warnings; the **Log Levels** panel counts every level.
- Numeric custom-level identifiers stay intact: `error2` is unclassified rather than normalized to `ERROR`.

### Time gaps

Zones are ignored. Supported ISO `Z`, `±HH:MM` and `±HHMM` offsets are normalized, so changing time-zone spelling alone can't create or hide a gap. Malformed offset suffixes yield no parsed time. Naive timestamps keep an assumed common local scale.

### Navigation

- Navigation needs the log inside the open project (the IDE only opens project files for plugins) and requires the editor owner and invoking project to match.
- Command and definition positions need an exact UTF-16-to-byte-to-UTF-16 roundtrip, so surrogate-interior or out-of-document carets can't silently select byte zero.
- Jump to Source scans up to 5,000 source files, as there is no index to query. It excludes files owned by another open project, including nested project roots. Equal best message/file ranks return no source jump instead of choosing the first file.
- Stack-frame definitions need an owned Log document (512 KiB maximum), parse at most 8 KiB per frame line, keep at most 32 frame observations and refuse zero or overflowing line numbers.
- File-name lookup is a suffix heuristic. Only a unique highest suffix score in an uncapped, project-owned regular-file candidate set is navigable; ambiguity returns no definition.

### Built-in Log editor overlay

- The API 1.3 semantic provider returns Ideolog's event, field and match tokens for the built-in Log editor, including captured timestamps, levels, messages and stack-trace continuation lines. Uncoloured entries keep the editor's existing styling.
- On v0.1.39 the built-in editor doesn't render the keyword/string/number/comment semantic kinds used by many rules. Built-in log colours alone aren't evidence that custom rules rendered.
- The overlay processes at most the first 64 KiB / 512 complete lines, stops before any line longer than 4096 bytes, and returns at most 4096 single-line UTF-16 tokens.
- It checks a 15 ms cooperative deadline during format attempts, physical-line grouping and lazy event parsing. Individual regex calls and rule passes can't be preempted.
- The newline search is bounded to a UTF-8-safe 64 KiB prefix before splitting lines. A prefix ending inside a line or Unicode scalar doesn't feed that partial line to format detection.
- A 512 KiB cutoff containing no complete line returns no custom syntax events, folds or outline entries; complete final document lines stay supported.
- Configuration is warmed outside the semantic request during activation, log focus/open and explicit pattern/capture work. Cold requests return no tokens, and external settings changes apply after that out-of-provider refresh.
- Providers do no project discovery, file reads, pattern-storage reads or network requests. The custom `ideolog` language keeps its own lexical highlighting, folding and outline path (512 KiB limit).

### Formats and Highlighting tab

- Format UUIDs stay internal identities. Duplicate display names are disambiguated. Controls revalidate the rendered storage/layout before mutation.
- Highlighting rules have a line/field/captured-match action, an optional column/format restriction (named format selector with Any log format; unknown imported restrictions stay labelled unavailable) and named colour choices.
- Preview works even when the draft's Enabled checkbox is off. Sample text stays in the draft and is never written to pattern storage. The sample has exactly two input paths: the bundled Example initializer and the explicit manual Sample field.
- **Save** validates regex syntax, capture-column bounds, actions and colour values. Editing preserves the UUID and other exported attributes, including `timePattern` and `fullmatch`.
- A changed storage snapshot blocks Save so another window's intervening edit is preserved. The SDK has no storage compare-and-swap, so this can't make concurrent writes atomic across instances. Draft fields carry a fresh edit-session identity; controls kept by another window after Cancel/reopen are rejected.
- Bounds: regex/attribute values 4096 bytes, format names 128 bytes, samples 8192 bytes with 4096-byte lines, saved storage 128 entries / 128 KiB. Samples use a bounded backtracking precheck; regex compilation and the parser aren't preemptible.
- Legacy pasted XML in `customPatterns` stays supported alongside the form.
- **Highlight Selection** doesn't import the text into the draft sample. Successful changes refresh only the Formats and Highlighting tab, and only when persistent pattern text actually changes. Cancelled, failed or no-op commands emit no false change. Notifications contain no definition or sample payload.

### Timestamp formats

- Supported: fixed numeric `yyyy`, `MM`, `dd`, `HH` (or `hh` with `a`), `mm`, `ss`, `SSS`, quoted literals and `Z`/`X`/`XX`/`XXX` offsets. A pattern needs hour, minute and second fields. Absent or empty keeps automatic parsing.
- `hh:mm:ss a` accepts case-insensitive ASCII `AM`/`PM` markers. Midnight, noon and time gaps use the same normalized scale as 24-hour formats. Ambiguous `hh` without `a`, `HH` with `a`, invalid twelve-hour values and extra text are rejected.
- Single-letter `M`, `d`, `H`, `h`, `m`, `s` accept one or two digits. `k`/`kk` accepts 1–24 (24 means midnight). `K`/`KK` accepts 0–11 with `a`. `S` reads one to three millisecond digits; `SS`/`SSS` exactly two and three (millisecond numbers, not decimal fractions). Variable-width adjacent numeric fields are rejected unless separated.
- `z` through `zzzz` accepts UTC, GMT and `GMT+02:00`-form offsets; region names aren't guessed. RFC822 `Z` through `ZZZ` keeps numeric `+0200` spelling.
- Unsupported tokens such as locale month/day names are rejected for new edits. Imported unsupported formats stay readable but produce configuration errors and no computed gap.
- Preview confirms whether the sample timestamp matches the format. The same parser drives the real time-gap calculation.

### Log Snapshot

- Capturing needs `editor.read`/`project.read` and an existing file in the invoking owning project. No outside-project grant, process or network is used. Rendering never rereads the source or starts capture.
- The snapshot reuses current format detection, event grouping, timestamp/category extraction and level classification. Literal captured levels such as INFO/DEBUG/custom names are kept; otherwise the level is Error/Warning/Info/Debug/Trace/Unclassified. The level filter counts and pages the kept events only.
- Messages are limited to 512 characters, timestamp and category to 128 each. Multiline events stay one row. Row navigation is separate from Jump to Source and doesn't invent resolved source links.
- Viewport requests page at most 100 rows (default 50), keeping position across tab switches. Row activation is restricted to the issued page and revalidates the current project and source text before opening.
- A capture accepts at most 256 KiB of current text and 8192 physical lines, keeps the first 2048 parsed events, and reports kept versus parsed count, source version, age and format. Oversized files or buffers are refused, not presented as complete.
- Discard releases the snapshot. Project switches don't expose another root's snapshot.
- Capture has a cancellable native task and a cooperative two-second parse deadline, with cancellation checkpoints between format-detection regex attempts and before every physical line during grouping. Cancellation returns no partial snapshot and earlier snapshots stay intact, but a single synchronous regex or read call can't be interrupted.
- This is snapshot paging, not byte-offset streaming, file tailing, appended-record following, rotation handling or durable background viewing. Pattern changes apply on the next capture; kept rows keep their capture-time parsing.

### JVM observations

- It runs an approved Java 17+ executable (`adapters/IdeologProbe.java`) for an explicitly selected custom `LogParsingPattern`, including raw imported patterns from `customPatterns` with Java regex constructs and `SimpleDateFormat` fields the local parser doesn't support.
- The review names the source, format, regex, date pattern, scope and local tool. No Java tool or application is installed or bootstrapped automatically. Syntax and semantic providers never launch a subprocess.
- The table shows event source lines, epoch milliseconds, time gaps, timestamp/severity/category/message captures and the actual Java version, locale and time zone. It follows Java `Pattern.find()` capture order, non-overlapping groups, multiline final-message extension and `SimpleDateFormat`'s default lenient, locale-aware parsing.
- Lines are limited to 4096 bytes. One bounded JVM child handles the whole snapshot: 64 MiB heap, 15-second deadline, cancellation, 256 KiB stdout / 8 KiB stderr. Display cells are truncated to 512 UTF-16 code units.
- Helper source and a bounded input file live in plugin-private storage. Input contents are cleared on every normal return or error (empty input-file metadata may remain until uninstall). The approved tool inherits the host's process environment.
- Missing or denied Java, unsupported versions, invalid patterns, timeouts and oversized output keep the previous observations. Activation rechecks project ownership, the exact current source, version where available and stored pattern configuration before navigating. Snapshots are memory-only. No log text is persisted or sent to a service.
- For a captured built-in Log document, cached observations supply semantic timestamp, recognized error/warning severity and category tokens, with real JVM time gaps driving the gap colour. A field is admitted only when it occurs exactly once in its original first physical line; repeated, multiline, possibly truncated and overlapping fields are withheld. Ranges use UTF-16 coordinates.
- Cache use requires the same document path, full text, version, deepest project owner, warmed custom-format configuration and current `customPatterns` setting. File, settings and focus events withdraw eligibility while keeping the table. Providers read neither files nor pattern storage and run no Java process. Stored format changes from another instance show after a configuration refresh or lifecycle event.
- Exact-source JVM observations can supply fold groups when the local parser can't express a custom format.

### Live Log Tail

- The Java helper (`adapters/IdeologTail.java`) uses secure directory-relative read handles and refuses symlinks. It starts within the last 64 KiB of the file and follows appends for at most 5 minutes / 1 MiB per explicitly started session.
- Each read is 4096 bytes. Only complete UTF-8 lines up to 4096 bytes are admitted. The view keeps at most 512 lines / 128 KiB. Filtering is case-insensitive and literal, and confined to the panel; it doesn't hide native editor lines.
- File identity changes and observed truncation reset the displayed generation. A missing replacement gets a 2-second rotation grace before stopping. Truncate-and-regrow between polls without changing identity or the kept prefix isn't guaranteed to be detected.
- The backend checks project ownership and unsaved-source state, bounds frames and output, rejects stale controller messages and stops its child on cancellation, source invalidation or plugin shutdown. The child self-expires even if the panel loses its controller. Source file contents are never written.
- Platforms without `SecureDirectoryStream` or usable file identity refuse the profile; no Windows profile is supplied.

### Folding, selection and marks

- The built-in **Log** language gets generic folding ranges for multiline events and stack traces.
- **Extend Log Selection** and the selection-range provider validate UTF-16 carets. Explicit selection changes recheck source version and text using the current editor API rather than file edits.
- **Mark Current Log Source** needs the log in an open project and `editor.read`/`project.read`/`events`. It examines at most the first complete 64 KiB / 512 lines (4096-byte line limit) and 4096 spans. Marks are cleared by the next semantic request (including edits), observed events, explicit Clear or plugin shutdown.
- Marks don't fix the semantic renderer, filter or redact log text, or provide arbitrary custom colours. API 1.4 source decorations expose no scrollbar heat-map contract.

</details>

## Development

    cargo test
    openidea-plugin build
    openidea-plugin link
