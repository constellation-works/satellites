# cw-app

The one path from any surface (a command line, an MCP server) to a program's commands: the
context a command runs in, the environment captured once, and an append-only audit log.
Mechanism only: it is generic over what the program built at start and names no store,
record, command or error code.

| Item | What it does |
|---|---|
| `App` | What a program supplies once: its state, commands, outcome and error; which commands are audited, how to run one, what an outcome acted on |
| `exec::<A>(runtime, origin, command)` | The chokepoint: starts the audit record, runs the command, translates its error into `cw_base::Error` once, returns a `Finished` |
| `Finished` | The result and the pending audit record: render the result, then `settle()`; a failure to write the line comes back as an `AuditWarning` value |
| `Runtime<S>` / `Ctx<'_, S>` | What the program built at start, and the borrowed view a command runs against: state, clock, origin |
| `Origin`, `Surface` | The surface (`cli`, `mcp`) and what the client said about itself, recorded as the actor |
| `Env::capture()` | The only read of the process environment; `Env::from_vars` for tests |
| `EnvVar`, `KIT_VARS`, `env::table(app, own)` | One declaration per variable read; the kit's own list; the Markdown usage table of both |
| `env.app(name).data_dir(explicit)` | Explicit root, else `<APP>_ROOT`, else `$HOME/.<app>`; a pure function of the captured value |
| `AuditLog` | `<root>/audit.jsonl`: append under its own lock, read within a cap |

## The chokepoint

```rust,ignore
let finished = exec::<Notes>(&runtime, &origin, command);
render(finished.result());               // the surface's own output
let (_result, warning) = finished.settle();
if let Some(warning) = warning { eprintln!("warning: {warning}") }
```

- A command whose `App::audit` returns `Some` gets its record before it runs, starting as a
  failure, so a panic is still logged as one. A record dropped without `settle` still writes.
- A read-only command (`None`) creates no file and takes no lock.
- The app's typed error reaches the caller as `cw_base::Error` with the app's kind and code,
  and the typed cause is recoverable with `downcast_ref`.
- The clock and the actor a command sees come from its `Ctx`; a test pins the clock with
  `Runtime::with_clock(Clock::fixed(..))`.

## The audit log

- **Identifiers only.** A line holds the time, surface, actor, command name, status,
  target, stable error code and duration: never an argument value or an error message.
  The actor is the client's claim, recorded only when it is an identifier (at most 64 bytes
  of ASCII letters, digits and `.` `_` `-` `@` `+` `:` `/`), else as `<invalid>`.
- **Its own lock.** Appends go through `cw_fs::append` under `<root>/.audit.lock`, never a
  program's own lock, so a command that timed out on that lock still records its failure.
- **Fails open.** A line that cannot be written never fails the command; the crate prints
  and logs nothing, and hands the failure to the surface as a warning.
- **Versioned.** Every line carries `format` (now 3). Formats 1 (no actor) and 2 (no
  surface) are read through declared upgrade steps; a newer line is refused in its place
  while the other lines are read. A torn last line is left out by the reader, and closed with a
  line break by the next append so that record stays whole. An append reads the log's last
  64 KiB (`cw_fs::read_tail`) under the lock and refuses a log whose newest line is newer than
  this build writes, or cannot be found there; the refusal is the surface's warning.

## The environment

`Env::capture()` is called once, in `main`. The presentation variables live in the same
snapshot: `NO_COLOR`, `CLICOLOR_FORCE`, `TERM`, `COLUMNS`, `<APP>_FORMAT`, `<APP>_LOG`. An empty
value reads as unset.
`Debug` shows variable names only, never values. `scripts/check-dependency-direction.sh`
fails when a file under `src/` outside `src/env/` names `std::env`; it scans `src/` only, so
tests and examples may read the environment.

## Declaring variables

Every variable a program reads is declared once, as a `const` `EnvVar`: its name (global, or
a suffix under the program's prefix), what it means, its default and the flag it stands in
for. The kit declares its own (`KIT_VARS`: the root, format and log variables, `NO_COLOR`,
`CLICOLOR_FORCE`, `TERM`, `COLUMNS`, `HOME`, `USER`, `USERNAME`) and reads them only through
those declarations; a program declares its own the same way and reads one with
`env.app(name).get(&VAR)`.

```rust,ignore
const STEP: EnvVar = EnvVar::app("STEP", "How much `add` adds.").with_default("`1`").with_flag("--step");
let step = env.app("tally").get(&STEP);           // TALLY_STEP
let usage = cw_app::env::table("tally", &[STEP]); // the kit's rows, then the program's
```

`table(app, own)` renders one Markdown table (variable, meaning, default, flag), the kit's
variables first and then the program's, in the order given, each on one row (a line break in
the text is written as `<br>`, a `|` escaped). It depends only on its
arguments, so a program's golden test pins its usage document to it: generate the section
from `table`, and a variable added, renamed or reworded without updating the document fails
the test.

## Errors

`cw_app::Error` has `kind() -> cw_base::Kind` and no code: the program that wraps it names its
own. It is not `#[non_exhaustive]`.

## Example

```sh
cargo run -p cw-app --example two_commands
```

## License

MIT.
