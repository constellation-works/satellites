# Architecture

The layer model that `scripts/check-dependency-direction.sh` enforces
(STD-02 R1, R6). A lower layer never imports a higher one. Adding a crate or
an edge changes this file and that script in the same change. Every crate
and top-level module has its place here, and every path written in code
formatting exists (`scripts/check-architecture-map.sh`, STD-02 R43).

The crates in this workspace are foundation and mechanism kit (STD-02 R4,
R5): reusable file, process, database, test and surface helpers that any
command-line program can use. They are not a layered product, so the
product layering rules bind on restructure rather than on this tree; the
standing note at the end records that.

## `cw-base` — shared error and clock

Owns the one error type every unit meets in plus a test-pinnable clock.
May import no other kit crate.

```
error/
time/
```

- `error/` — the one error type a program's units meet in: a unit's typed
  error kept whole with its kind and stable code.
- `time/` — time as a program records it: a clock a test can pin and one
  timestamp format.

## `cw-fs` — private file access and a lock

Owns file access for a program's private data directory: durable writes,
bounded reads, owner-only checks and one advisory lock with a deadline.
Mechanism only; it knows no record and no file name. May import `cw-base`.

```
dir/
error/
file/
inspect/
lock/
```

- `dir/` — directories: created owner-only, listed in order, flushed after
  a change.
- `error/` — what a file operation fails with.
- `file/` — files: replaced whole, created once, appended to, read within a
  cap, removed.
- `inspect/` — checking existing state before it is trusted.
- `lock/` — one exclusive advisory lock, with a deadline.

## `cw-testkit` — test isolation

Owns test isolation for command-line tools: a private temporary root, a
child command built from a cleared environment, and a golden-file harness.
It is a dev-dependency of the other crates' tests and of downstream
projects. May import no other kit crate.

```
child/
error/
golden/
root/
```

- `child/` — the child command built from a cleared environment.
- `error/` — the one error type of the crate.
- `golden/` — the golden harness: output compared with checked-in files.
- `root/` — the private temporary root a test runs in.

## `cw-supervise` — bounded child runs

Owns running a child process within bounds: in its own process group, with
a required deadline, a polite-then-forced stop, a sweep of the whole group
on every exit path, and capped output. May import no other kit crate.

```
env/
error/
run/
```

- `env/` — clear a child's environment and pass only what the caller names.
- `error/` — what a run fails with.
- `run/` — one bounded run of a child process: the builder and what it
  returns.

## `cw-sqlite` — one private SQLite database

Owns one private SQLite database for a command-line tool: a writer or a
reader opened per call, fixed pragmas, one write transaction, and an
append-only list of migrations keyed on the schema version. Mechanism only;
it knows no table and no record. May import `cw-base` and `cw-fs`.

```
connection/
db/
error/
migration/
test_support/
```

- `connection/` — opening a connection: a writer for writes and a reader
  for reads; no pool.
- `db/` — the database a project names once, and what it does.
- `error/` — what a database operation fails with.
- `migration/` — the schema as an ordered, append-only list of steps.
- `test_support/` — for a project's own tests, behind the feature of the
  same name: a database in a temporary root, and staging at an older schema
  version.

## `cw-app` — the command chokepoint

Owns the one path from any surface to a program's commands: the context a
command runs in, the environment captured once, and an append-only audit
log. May import `cw-base` and `cw-fs`.

```
audit/
env/
error/
exec/
```

- `audit/` — the audit log: one line per mutating command.
- `env/` — the process environment, read once.
- `error/` — what this crate fails with.
- `exec/` — the chokepoint: the one path from any surface to a program's
  commands.

## `cw-cli` — command-line surface mechanism

Owns what every command line repeats, as mechanism: one terminal decision,
a sink whose standard output carries only the payload, error rendering with
the exit code from its kind, logging to standard error, and grouped root
help. May import `cw-base` and `cw-app`.

```
error/
help/
logging/
output/
terminal/
```

- `error/` — how a failure is shown and what the process exits with, and
  the typed error this crate fails with.
- `help/` — the root help, grouped, and the check that keeps its summaries
  short.
- `logging/` — tracing to standard error, at the level the program log
  variable names.
- `output/` — the sink: the only path from a command to standard output and
  standard error.
- `terminal/` — the one terminal decision: colour, width and output mode.

## `cw-mcp` — Model Context Protocol server

Owns a Model Context Protocol server over standard input and output,
serving a program's commands as a list of typed tools through the same
entry point its command line calls. May import `cw-base` and `cw-app`.
The protocol SDK and the async runtime stay private inside `wire/` and
appear in no public signature.

```
error/
payload/
server/
testing/
tool/
wire/
```

- `error/` — what this crate fails with.
- `payload/` — what a client receives from a tool call: a wire contract.
- `server/` — the server: a checked list of tools over a program's runtime,
  served over standard input and output.
- `testing/` — an in-process harness for wire tests, behind the feature of
  the same name.
- `tool/` — a tool: what a client can call, and the function that answers
  it.
- `wire/` — the protocol and the runtime, kept private: the only folder
  that names the SDK or the runtime.

## Standing against the v5 and v6 architecture rules

This is a standing note, not a deviation record. Per Adopting v5 and
Adopting v6 in STD-02, the R39 to R44 rules with the v5 forms of R1, R2, R6
and R7, and the R45 to R47 rules with the v6 form of R7, bind a new project
and any restructuring of an existing one; this workspace records where it
stands until it restructures and is not required to restructure in order to
re-pin.

The crates here are foundation and mechanism kit (R4 and R5), not a layered
product: `cw-base` holds the shared error and clock with no file,
environment or process use, and `cw-fs`, `cw-sqlite`, `cw-supervise`,
`cw-testkit`, `cw-app`, `cw-cli` and `cw-mcp` hold reusable mechanisms with
no product domain, operation or composition of their own. The product
layering rules therefore have no product layer to bind yet: there is no
domain crate holding records and rules, no application crate holding one
entry point per subject, and no composition crate joining configuration and
adapters.

What holds now: the crate graph is acyclic and follows the declared order,
checked by `scripts/check-dependency-direction.sh`; `cw-sqlite` is the
storage mechanism unit and `cw-fs` the file mechanism unit, each importing
contract types only in the R40 direction; and the unit map itself is checked
by `scripts/check-architecture-map.sh`.

What a future restructure must meet: put records and pure rules in domain
units (R2, R40, R41), one entry point per operation in application units
sliced by subject (R2, R45), exactly one composition unit that only
constructs (R2), surfaces that only parse, call one operation and render
(R2), ports defined by the caller and implemented in composition or an
adapter (R42), legacy and upgrade readers quarantined to one unit called
only at open or upgrade (R44), no model or agent provider calls in a tool
(R46), subjects kept whole instead of scattered to dodge input and output
(R47), no cycles at any level with an order declared inside any layer past
about ten units (R39), and crate splits justified by a build or dependency
need with persistence as its own crate from the day a storage-driver
dependency is taken (R7).
