//! Pragmas, read and set with plain SQL statements.
//!
//! `rusqlite`'s `pragma_update` reads the schema before it sets a value, and
//! so fails on a database in a read-only directory; a plain `PRAGMA` does
//! not. Names and values here are this crate's own constants and integers,
//! never a caller's text.

use rusqlite::Connection;
use rusqlite::types::FromSql;
use std::fmt::Display;

/// `PRAGMA name = value`, stepping through any row it returns.
pub(crate) fn set(conn: &Connection, name: &str, value: impl Display) -> rusqlite::Result<()> {
    let mut statement = conn.prepare_cached(&format!("PRAGMA {name} = {value}"))?;
    let mut rows = statement.query([])?;
    while rows.next()?.is_some() {}
    Ok(())
}

/// `PRAGMA name`, or `PRAGMA name = value` when `value` is given, and the
/// first column of the row it returns.
pub(crate) fn get<T: FromSql>(
    conn: &Connection,
    name: &str,
    value: Option<&str>,
) -> rusqlite::Result<T> {
    let sql = match value {
        Some(value) => format!("PRAGMA {name} = {value}"),
        None => format!("PRAGMA {name}"),
    };
    conn.prepare_cached(&sql)?.query_row([], |row| row.get(0))
}
