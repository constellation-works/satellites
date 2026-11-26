//! A small notes store: append-only migrations, one write transaction, a
//! read-only reader, a strict read that creates and changes no file, and an
//! integrity check.
//!
//! It shows the conventions the kit leaves to a project: `STRICT` tables,
//! RFC 3339 UTC text timestamps, and JSON in a text column with a
//! `json_valid` check.
//!
//! Run with `cargo run -p cw-sqlite --example notes`. It works in a
//! temporary directory and removes it on exit.

use cw_sqlite::rusqlite::Transaction;
use cw_sqlite::{Db, Error, Step};
use std::io::Write as _;
use std::time::Duration;

/// Append-only: a released step is never edited; a change is a new step.
const MIGRATIONS: &[Step] = &[
    Step::sql(
        "CREATE TABLE note (
            id      INTEGER PRIMARY KEY,
            created TEXT NOT NULL,
            body    TEXT NOT NULL
        ) STRICT;",
    ),
    Step::sql(
        "ALTER TABLE note ADD COLUMN tags TEXT NOT NULL DEFAULT '[]'
            CHECK (json_valid(tags));",
    ),
    Step::function(index_tags),
];

fn index_tags(tx: &Transaction<'_>) -> cw_sqlite::rusqlite::Result<()> {
    tx.execute_batch("CREATE INDEX note_created ON note (created);")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scratch = tempfile::tempdir()?;
    let db = Db::new(scratch.path().join("notes.db"), MIGRATIONS).lock_wait(Duration::from_secs(2));

    db.write(|tx| {
        let mut insert =
            tx.prepare_cached("INSERT INTO note (created, body, tags) VALUES (?1, ?2, ?3)")?;
        insert.execute(("2026-11-24T22:00:00Z", "first note", r#"["kit"]"#))?;
        insert.execute(("2026-11-24T22:05:00Z", "second note", "[]"))?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;

    let reader = db.read()?;
    let mut out = std::io::stdout().lock();
    writeln!(out, "schema version {}", reader.version())?;
    let mut notes = reader
        .prepare_cached("SELECT created, body FROM note ORDER BY created")
        .map_err(|e| Error::sqlite("list the notes in", db.path(), e))?;
    let rows = notes
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| Error::sqlite("list the notes in", db.path(), e))?;
    for row in rows {
        let (created, body) = row.map_err(|e| Error::sqlite("list the notes in", db.path(), e))?;
        writeln!(out, "{created}  {body}")?;
    }
    db.check()?;
    writeln!(out, "quick_check: ok")?;

    // A command that must create and change no file reads strictly. The
    // writer above left its `-wal` and `-shm`, so this succeeds; with either
    // missing it is refused with `Error::WalUnavailable`, never served from
    // the main file alone.
    let strict = db.read_strict()?;
    let count: i64 = strict
        .query_row("SELECT count(*) FROM note", [], |row| row.get(0))
        .map_err(|e| Error::sqlite("count the notes in", db.path(), e))?;
    writeln!(
        out,
        "strict read: {count} notes, no file created or changed"
    )?;
    Ok(())
}
