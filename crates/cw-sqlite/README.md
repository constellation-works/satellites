# cw-sqlite

One private SQLite database for a command-line tool: a writer or a reader opened per call,
fixed pragmas, one `IMMEDIATE` write transaction, and an append-only list of migrations.
Mechanism only: it knows no table and no record. It re-exports `rusqlite` (with SQLite
bundled), so a project names one version of it.

```rust
let db = Db::new(path, MIGRATIONS).lock_wait(wait);
db.write(|tx| { … })?;      // BEGIN IMMEDIATE; commits on Ok, rolls back on Err
let conn = db.read()?;      // read-only; writes no row
db.check()?;                // PRAGMA quick_check, for a doctor command
```

Sized for a command-line tool: there is no pool, and each call opens its own connection.

## What it fixes

| Matter | What it does |
|---|---|
| Before opening | The directory is resolved, and the resolved path is what is inspected, created and opened; the directory itself is trusted, as in `cw-fs`. The file and its `-wal` and `-shm` are inspected through `cw-fs`: a link, another user's file or one with a group or other permission bit is refused. A new database is created `0600` before SQLite sees it. A path that is not valid UTF-8, as given or once resolved, is refused |
| Open flags | Read-write, no create, no mutex, no following a link |
| Journal mode | WAL. The switch's waits, through the busy handler and through the crate's retry of a busy result, end at the call's deadline; a database that stays out of WAL is an error |
| `synchronous` | `FULL`, so a committed transaction survives power loss, not only a crash |
| `foreign_keys` | On. A migration step can declare it off while it runs |
| WAL files after the last close | Kept, so a read-only reader finds them |
| Setting a pragma | Plain SQL, not `rusqlite`'s helper, which reads the schema and fails in a read-only directory |
| Lock wait | One deadline for the whole call: the lock wait after the call started, by the clock (`Instant`), at most `i32::MAX` milliseconds (about 24.8 days), as `busy_timeout` capped it. It bounds the waits that go through the connection's busy handler: a lock for the version check before a write and for the writer's own version read, each migration step, and the write lock (`BEGIN IMMEDIATE`); the switch to WAL, whose waits end at the same deadline whether SQLite waits through the busy handler (as it can when another connection holds a rollback-journal database) or returns a busy result, which the crate retries. One call can wait at several of those points, and all of them end at the same deadline, so the call stops waiting close to one lock wait after it started, past it by at most the last step's oversleep and one retry; a wait that begins after the deadline fails at once. The busy handler is the crate's own, not SQLite's `busy_timeout`, which counts its planned sleeps and never reads the clock: it reads the clock before each retry, sleeps short steps (1 to 16 ms) that end no later than the deadline, and gives up once it has passed, with `Error::Locked`. Outside the deadline is SQLite's own retry for a WAL read mark (`walTryBeginRead`), which never calls the busy handler: it backs off on its own schedule and, when its retries run out, fails with `SQLITE_PROTOCOL`, reported as `Error::Sqlite`, not `Error::Locked`. A `Reader` keeps the deadline of the `read` call that opened it; inside its snapshot it never waits |
| Write transactions | Always `IMMEDIATE`, so a write never meets a busy snapshot halfway through. The schema version is checked again under the write lock, before the callback runs. The callback must not run `COMMIT` or `ROLLBACK`; a write whose transaction it ended fails |
| Readers | Read-only and `query_only`; a reader writes no row and never checkpoints on close. It holds one read transaction, opened by its version check, so everything it reads is from the version it checked, until the caller ends that transaction. It holds no lock that blocks a writer, except briefly: on the first attach in a process to a WAL whose index is not initialised, SQLite's own recovery may take its recovery and write locks, even for a reader, and a writer meeting that waits within the lock wait. SQLite maps the `-shm` index once per process, so a reader opens it as a writer in the same process needs it, by the directory. In a directory this process can write, the reader shares the index: SQLite may rebuild the `-shm` (its index, not database content) and creates a missing `-wal` (empty) and `-shm`, owner-only; where a `-shm` this process cannot write rules that out, the index is opened read-only and SQLite may still create a missing `-wal`, owner-only. In a directory this process cannot write (a copy, a read-only mount), a reader writes nothing: the index is opened read-only, and when the WAL cannot be read that way the main file is opened `immutable=1` and `Reader::immutable()` says so, or the read is refused when the `-wal` has content. SQLite's WAL open asks for read-write on an existing owner-writable `-wal`, falling back to read-only, so it may hold one open read-write; a read-only connection writes nothing through it |
| Schema version | `PRAGMA user_version` |
| Migrations | An ordered, append-only list of `Step`s, SQL or a function. Each runs in an `IMMEDIATE` transaction that re-reads the version first, so two processes racing on a new database each run a step once; a failing step leaves the version and the schema as they were. A step must not contain transaction control: one that ends its transaction fails and the version is not advanced, but what it committed itself stays |
| A database newer than the binary | Refused for reads and writes: a writer checks through a reader before its read-write open, and again under the write lock. The refusal writes no database content and leaves the main file and any existing `-wal` data alone, though in a writable directory SQLite may rebuild the `-shm`, its index, and may create a missing `-wal`, empty and owner-only, before the version check refuses; a refused connection that closes last may still checkpoint committed WAL pages into the main file, which changes neither content nor version |
| Statements | The crate's own use `prepare_cached` |
| `secure_delete` | Off unless `Db::secure_delete(true)`: for a project that promises to forget deleted text |

Conventions the example shows, with no code here: `STRICT` tables, RFC 3339 UTC text
timestamps, JSON in text columns with a `json_valid` check.

## Errors

`cw_sqlite::Error` has `kind() -> cw_base::Kind` and no code: the unit that wraps it names
its own. Busy or locked is `Locked` (`Kind::Busy`); not-a-database or damaged is `Corrupt`;
a newer schema is `Incompatible`; the rest keep the action, the path and SQLite's error.
`Locked` names no holder: SQLite's locks record no owner, and recording one would be a write
by every connection, readers included.
`Error::sqlite(action, path, e)` classifies a project's own statement failures the same way.
It is not `#[non_exhaustive]`.

## Testing

The `test-support` feature adds `test_support::temp_db`, a database in a temporary root
created `0700`, and `test_support::stage`, which creates a database at an older schema
version so a test can exercise the later steps alone.

## Left out

A reader pool, savepoints, a backup before migrating, and per-feature migration ledgers: a
command-line tool that does one store write per command needs none of them.

## Platforms

Unix and Windows. On Windows the owner and mode checks are skipped; a symbolic link at the
database or a sidecar is still refused.

## Example

```sh
cargo run -p cw-sqlite --example notes
```

## License

MIT.
