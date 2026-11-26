# cw-cli

What every command line repeats: one terminal decision, a sink whose stdout carries only the
payload, error rendering with the exit code from its kind, logging to stderr, and grouped
root help. Mechanism only: it is parameterised by the program's name, names no command,
record or error code, and reads no environment variable itself; the program passes the
snapshot `cw_app::Env::capture()` took.

| Item | What it does |
|---|---|
| `Terminal::detect(&env, app, format)` | The one decision, made once: colour for stdout and stderr, the width a table fits, the output mode |
| `Terminal::decide(&env, app, probe, format)` | The same decision as a pure function of the snapshot and a `Probe` of the streams, for tests |
| `Format` | `auto`, `table` or `json`: a `clap::ValueEnum` for the program's `--format` flag |
| `Sink::stdio(terminal)` / `Sink::new(terminal, out, err)` | The only path to stdout and stderr |
| `sink.emit(&payload, to_table)` | One payload, rendered as a borderless table, tab-separated lines or one JSON document |
| `sink.emit_text(&payload, to_text)` | The same for a detail view that is not a list; escapes except line breaks and tabs |
| `sink.notice(text)` / `sink.warn(text)` | One escaped line on stderr, in every mode |
| `sink.notice_lines(lines)` / `sink.warn_lines(lines)` | One escaped line per entry on stderr, the only multi-line stderr |
| `sink.finish()` | Flush stdout once the command is done; a closed reader is success |
| `sink.report(&error, hint)` | A `cw_base::Error` on stderr, and the `Exit` its kind calls for |
| `exit_code(kind)` | 2 for `Kind::Usage`, 1 for every other kind |
| `Logging::from_env(&env, app).init()` | `tracing` to stderr at the level `<APP>_LOG` names, `warn` by default |
| `help::grouped(cmd, groups, width)` | The root help with commands under headings, one-line summaries |
| `help::check_summaries(&cmd, SUMMARY_LIMIT)` | The check a program's test calls: every summary is one line of at most 70 characters |

## The terminal decision

Made once, in `src/terminal/`, from the captured environment and `isatty`; no other file
asks, and `scripts/check-dependency-direction.sh` fails when one does.

- **Colour** is off when the stream is not a terminal, when `TERM=dumb`, and when
  `NO_COLOR` is non-empty. A non-empty `CLICOLOR_FORCE` turns it on for a terminal, a dumb
  one included, but never over `NO_COLOR` and never for a pipe. stdout and stderr are
  decided apart, so `tool | less` still colours an error on the terminal.
- **Width** is known only when stdout is a terminal: `COLUMNS`, else the terminal's own size
  (Unix). With no width a table is never cut.
- **Mode**: the flag, then `<APP>_FORMAT`, then `auto`. `auto` is a table on a terminal and
  tab-separated lines otherwise. An unrecognised variable value falls back to `auto`.

## Output

- stdout carries the payload and nothing else; notices, warnings and errors go to stderr in
  every mode.
- A table is borderless: one header row, exactly one line per record, two-space gutters,
  numbers right-aligned (`Column::right`), `-` for an absent cell. Widths are measured in
  display columns, so a wide character counts two. Only a column marked `truncatable` is
  cut, only on a terminal, and a cut value ends in a single `…`.
- A list with no records writes nothing to stdout in a table or piped (no lone header) and
  `[]` in JSON; the line on stderr naming what was searched is the program's.
- Piped, the same rows are tab-separated lines with no header.
- Every control character in a value is written as a visible escape (`\n`, `\t`, `\r`,
  else `\u{...}`), so a value cannot break its line or reach the terminal as control.
  JSON escapes its own.
- `notice`, `warn` and `report` escape every control character in their text the same
  way (`\n`, `\t`, `\r`, else `\u{...}`): a line feed, carriage return, tab or ESC in
  the argument comes out escaped, never as control, so a value cannot forge a second
  `error:` or `warning:` line. Several stderr lines need `notice_lines`/`warn_lines`,
  one escaped line per entry; passing raw text with a line feed never writes two lines.
  `emit_text` instead keeps line breaks and tabs for the detail view and escapes the
  rest.
- Colour is one of six roles (`Role`): ok, warn, error, active, muted, neutral, each a basic
  16-colour code. The program maps its values onto roles in one table of its own.
- **A closed stdout** (`tool list | head -1`) latches the sink shut: later writes do
  nothing, the command runs to its end (its commit included), and `finish` reports success,
  so the process exits 0 with nothing on stderr. Any other write failure comes back from
  `finish` as `Error::Output`, once the command is done.

## Errors

```rust,ignore
let outcome = run(&mut sink).and_then(|()| sink.finish().map_err(MyError::from));
let exit = match outcome {
    Ok(()) => Exit::SUCCESS,
    Err(error) => sink.report(&cw_base::Error::from(error), hint),
};
ExitCode::from(exit)
```

- Text: the first line is `error: <message>`, the message being the error's sentence and
  each cause after it; a known fix follows on a `hint:` line.
- JSON mode: one object on one line, `{"error": ..., "code": ..., "hint": ...}`, where
  `hint` is `null` when there is none and `code` is the program's own stable code.
- The exit code is 2 for `Kind::Usage` and 1 for every other kind, in one exhaustive match.
- The crate's own `Error` has a `kind()` and no code: the program wraps it and names one.

## Logging

`Logging::from_env(&env, app)` reads `<APP>_LOG` from the snapshot as a `tracing` filter
(`info`, `notes=debug`). Unset or empty means `warn`. A value that does not parse never
fails the command: the level stays `warn` and `warning()` says why; `init()` prints that
warning and installs the subscriber on stderr. That warning is the one line the crate
writes to stderr directly rather than through tracing or a `Sink`: no subscriber exists yet
to carry it, and the sink is built later, from flags parsed after logging starts.
`subscriber(writer)` builds the same subscriber on another writer, for a test.

## Help

`help::grouped` keeps clap's usage, options and footer and replaces its flat command list
with the program's groups, each command on one line with its summary, wrapped to the
terminal's width when there is one. Build clap without its `color` and `wrap_help`
features: both make clap ask the terminal on its own. `help::check_summaries` walks the
whole command tree and fails on any command whose summary is empty or longer than the
limit; a program's own test calls it on its assembled parser.

Left out: NDJSON streaming, log files and rotation, and a pager.

## Example

`cargo run -p cw-cli --example notes -- list` runs a two-command program on the crate; the
example's header lists more to try.
