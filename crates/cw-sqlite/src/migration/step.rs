//! One schema step.

use rusqlite::Transaction;
use std::fmt;

/// One migration step: SQL text, or a function over the open transaction.
///
/// A step must not contain transaction control (`BEGIN`, `COMMIT`,
/// `ROLLBACK`): its transaction is the migration's, which records the new
/// version in it. A step that ends it fails, with
/// [`crate::Error::Migration`], and the version is not advanced; what it
/// committed before ending it stays.
///
/// ```
/// use cw_sqlite::Step;
///
/// fn backfill(tx: &cw_sqlite::rusqlite::Transaction<'_>) -> cw_sqlite::rusqlite::Result<()> {
///     tx.execute_batch("UPDATE note SET title = '' WHERE title IS NULL;")
/// }
///
/// const MIGRATIONS: &[Step] = &[
///     Step::sql("CREATE TABLE note (id INTEGER PRIMARY KEY, title TEXT) STRICT;"),
///     Step::function(backfill),
/// ];
/// # let _ = MIGRATIONS;
/// ```
#[derive(Clone, Copy)]
pub struct Step {
    body: Body,
    foreign_keys_off: bool,
}

/// What a step runs.
#[derive(Clone, Copy)]
pub(crate) enum Body {
    /// A batch of SQL statements.
    Sql(&'static str),
    /// A function over the step's transaction.
    Function(fn(&Transaction<'_>) -> rusqlite::Result<()>),
}

impl Step {
    /// A step that runs `sql`, one or more statements, as a batch.
    #[must_use]
    pub const fn sql(sql: &'static str) -> Self {
        Self {
            body: Body::Sql(sql),
            foreign_keys_off: false,
        }
    }

    /// A step that calls `apply` with its transaction, for a change SQL
    /// alone cannot express. An error from it rolls the step back.
    #[must_use]
    pub const fn function(apply: fn(&Transaction<'_>) -> rusqlite::Result<()>) -> Self {
        Self {
            body: Body::Function(apply),
            foreign_keys_off: false,
        }
    }

    /// Run this step with `foreign_keys` off, for a step that rebuilds a
    /// table others reference. SQLite ignores the pragma inside a
    /// transaction, so it is turned off before the step's transaction begins
    /// and on again after it ends, whether it committed or not. The step
    /// should run `PRAGMA foreign_key_check` itself before it returns.
    #[must_use]
    pub const fn foreign_keys_off(self) -> Self {
        Self {
            foreign_keys_off: true,
            ..self
        }
    }

    /// Whether this step runs with `foreign_keys` off.
    #[must_use]
    pub const fn runs_with_foreign_keys_off(&self) -> bool {
        self.foreign_keys_off
    }

    pub(crate) fn apply(&self, tx: &Transaction<'_>) -> rusqlite::Result<()> {
        match self.body {
            Body::Sql(sql) => tx.execute_batch(sql),
            Body::Function(apply) => apply(tx),
        }
    }
}

impl fmt::Debug for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let body = match self.body {
            Body::Sql(sql) => sql,
            Body::Function(_) => "<function>",
        };
        f.debug_struct("Step")
            .field("body", &body)
            .field("foreign_keys_off", &self.foreign_keys_off)
            .finish()
    }
}
