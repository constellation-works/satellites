# cw-sqlite

One private SQLite database for a command-line tool: a writer or a reader opened per call,
fixed pragmas, one `IMMEDIATE` write transaction, and an append-only list of migrations.
Mechanism only: it knows no table and no record. It re-exports `rusqlite` (with SQLite
bundled), so a project names one version of it.

```rust
let db = Db::new(path, MIGRATIONS).lock_wait(wait);
db.write(|tx| { … })?;      // BEGIN IMMEDIATE; commits on Ok, rolls back on Err
let conn = db.read()?;      // read-only; takes no lock, writes nothing
db.check()?;                // PRAGMA quick_check, for a doctor command
```

Sized for a command-line tool: there is no pool, and each call opens its own connection.

## What it fixes

| Matter | What it does |
|---|---|
| Before opening | The directory is resolved, and the resolved path is what is inspected, created and opened; the directory itself is trusted, as in `cw-fs`. The file and its `-wal` and `-shm` are inspected through `cw-fs`: a link, another user's file or one with a group or other permission bit is refused. A new database is created `0600` before SQLite sees it. A path that is not valid UTF-8 is refused |
| Open flags | Read-write, no create, no mutex, no following a link |
| Journal mode | WAL. The switch is retried until the lock wait runs out, because SQLite does not call its busy handler for it; a database that stays out of WAL is an error |
| `synchronous` | `FULL`, so a committed transaction survives power loss, not only a crash |
| `foreign_keys` | On. A migration step can declare it off while it runs |
| WAL files after the last close | Kept, so a read-only reader finds them |
| Setting a pragma | Plain SQL, not `rusqlite`'s helper, which reads the schema and fails in a read-only directory |
| `busy_timeout` | The lock wait |
| Write transactions | Always `IMMEDIATE`, so a write never meets a busy snapshot halfway through. The schema version is checked again under the write lock, before the callback runs. The callback must not run `COMMIT` or `ROLLBACK`; a write whose transaction it ended fails |
| Readers | Read-only and `query_only`, with the `-shm` index opened read-only too. A reader holds one read transaction, opened by its version check, so everything it reads is from the version it checked. When the WAL cannot be read and holds nothing (a copy, a read-only mount), the main file is opened `immutable=1` and `Reader::immutable()` says so; when the WAL holds pages the main file may lack, the read is refused. A read creates and changes no file, except an empty owner-only `-wal` for a WAL-mode database that has a `-shm` and no `-wal`, which SQLite gives no way to avoid |
| Schema version | `PRAGMA user_version` |
| Migrations | An ordered, append-only list of `Step`s, SQL or a function. Each runs in an `IMMEDIATE` transaction that re-reads the version first, so two processes racing on a new database each run a step once; a failing step leaves the version and the schema as they were. A step must not contain transaction control: one that ends its transaction fails, and the version is not advanced |
| A database newer than the binary | Refused for reads and writes, and left unchanged: a writer checks the version before it opens the database for writing. The one exception is a `-wal` with pages and no `-shm`, where only a read-write open can read the version, and it recreates the `-shm` |
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
