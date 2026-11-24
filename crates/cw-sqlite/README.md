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
| Before opening | The file and its `-wal` and `-shm` are inspected through `cw-fs`: a link, another user's file or one with a group or other permission bit is refused. A new database is created `0600` before SQLite sees it |
| Open flags | Read-write, no create, no mutex, no following a link |
| Journal mode | WAL. The switch is retried until the lock wait runs out, because SQLite does not call its busy handler for it; a database that stays out of WAL is an error |
| `synchronous` | `FULL`, so a committed transaction survives power loss, not only a crash |
| `foreign_keys` | On. A migration step can declare it off while it runs |
| WAL files after the last close | Kept, so a read-only reader finds them |
| Setting a pragma | Plain SQL, not `rusqlite`'s helper, which reads the schema and fails in a read-only directory |
| `busy_timeout` | The lock wait |
| Write transactions | Always `IMMEDIATE`, so a write never meets a busy snapshot halfway through |
| Readers | Read-only and `query_only`; the `-shm` index is opened read-only too, so a read changes no file. When the WAL cannot be read (a copy on a read-only mount), retried as `immutable=1`, and `Reader::immutable()` says so |
| Schema version | `PRAGMA user_version` |
| Migrations | An ordered, append-only list of `Step`s, SQL or a function. Each runs in an `IMMEDIATE` transaction that re-reads the version first, so two processes racing on a new database each run a step once; a failing step leaves the version and the schema as they were |
| A database newer than the binary | Refused for reads and writes, and left unchanged |
| Statements | The crate's own use `prepare_cached` |
| `secure_delete` | Off unless `Db::secure_delete(true)`: for a project that promises to forget deleted text |

Conventions the example shows, with no code here: `STRICT` tables, RFC 3339 UTC text
timestamps, JSON in text columns with a `json_valid` check.

## Errors

`cw_sqlite::Error` has `kind() -> cw_base::Kind` and no code: the unit that wraps it names
its own. Busy or locked is `Locked` (`Kind::Busy`); not-a-database or damaged is `Corrupt`;
a newer schema is `Incompatible`; the rest keep the action, the path and SQLite's error.
`Error::sqlite(action, path, e)` classifies a project's own statement failures the same way.
It is not `#[non_exhaustive]`.

## Testing

The `test-support` feature adds `test_support::temp_db`, a database in a temporary
owner-only root, and `test_support::stage`, which creates a database at an older schema
version so a test can exercise the later steps alone.

## Left out

A reader pool, savepoints, a backup before migrating, and per-feature migration ledgers: a
command-line tool that does one store write per command needs none of them.

## Platforms

Unix and Windows. On Windows the owner, mode and link checks are skipped.

## Example

```sh
cargo run -p cw-sqlite --example notes
```

## License

MIT.
