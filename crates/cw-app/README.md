# cw-app

The one path from any surface (a command line, an MCP server) to a program's commands: the
context a command runs in, the environment captured once, and an append-only audit log.
The crate is generic over what the program built at start, including its store, records,
commands and command-specific error codes.

| Item | What it does |
|---|---|
| `App` | What a program supplies once: its state, commands, outcome and error; which commands are audited, how to run one, what an outcome acted on, optionally which record a command is aimed at |
| `exec::<A>(runtime, origin, command)` | Appends a started line before running a mutating command, then returns a `Finished` |
| `Finished` | Render the result, then call `settle()` to append the outcome. An outcome append error comes back as an `AuditWarning` |
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

- A command whose `App::audit` returns `Some` runs only after the started line is appended
  and synced. If the append fails, `Finished::result` returns `audit_start_failed` with
  the typed `cw_app::Error` cause. An ID generation failure returns `audit_id_failed`.
- After rendering, `settle` appends the outcome with the same ID. Dropping a pending
  outcome still attempts to append it if the command returned.
- A read-only command (`None`) creates no file and takes no lock.
- The app's typed error reaches the caller as `cw_base::Error` with the app's kind and code,
  and the typed cause is recoverable with `downcast_ref`.
- The clock and the actor a command sees come from its `Ctx`; a test pins the clock with
  `Runtime::with_clock(Clock::fixed(..))`.

## The audit log

- **Identifiers only.** A line holds the time, surface, actor, command name, status,
  target, stable error code and duration: never an argument value or an error message.
  The actor is the client's claim, recorded only when it is an identifier (at most 64 bytes
  of ASCII letters, digits and `.` `_` `-` `@` `+` `:` `/`), else as `<invalid>`. The target
  is the app's, under the same rule (at most 128 bytes), else `<invalid>`.
- **The target is known before the command runs.** An app may implement `App::subject`: the
  id of the record a mutating command is aimed at, resolved against its own state (a read,
  before the started line; `None` by default). The started line carries it as the target and
  the outcome line keeps it on failure, so a refused write and a start with no outcome
  still name their record. On success `App::target(outcome)` wins when it names a record
  (a newly created id); otherwise the subject stays. Return a resolved id, never the selector
  the user typed.
- **Its own lock.** Appends go through `cw_fs::append` under `<root>/.audit.lock`, never a
  program's own lock, so a command that timed out on that lock still records its failure.
- **Write-ahead.** A started append failure stops the command. An outcome append failure
  returns a warning and preserves the command's result. The reader pairs lines by ID and
  returns one record per command in start order. A start with no outcome has `Status::Unknown`
  and no duration. A completed record reports its duration in `Record::duration_ms` as `Some`.
- **Versioned.** Format 4 adds `phase`, with `started` and `outcome` lines sharing an `id`.
  Formats 1 through 3 upgrade to standalone records, as do direct `AuditLog::append` calls
  when written in format 4. A newer line is refused in its place while other lines are read.
  A torn last line is left out by the reader and closed with a line break by the next append.
  An append reads the last 64 KiB under the lock and refuses a log whose newest readable
  format is newer than this build or cannot be found in a tail cut from a longer log.

For the next `rust-cli-template` pin, account for `Status::Unknown` and the optional
`Record::duration_ms` in readers. Builds that write format 3 refuse to append once the log
contains a format 4 line as its newest readable line.

Read the log back using only `cw_app` paths, including its byte cap:

```rust,ignore
use cw_app::{AuditLog, Cap};

let log = AuditLog::new(root);
for record in log.read(Cap::bytes(1 << 20))? {
    let record = record?;
    println!("{} {:?}", record.command, record.status);
}
```

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
