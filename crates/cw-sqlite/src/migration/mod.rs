//! The schema, as an ordered, append-only list of [`Step`]s keyed on
//! `PRAGMA user_version`.
//!
//! Step `n` (1-based) moves the database from version `n - 1` to `n`. A step
//! is never edited or removed once released; a change is a new step at the
//! end. Each step runs in its own `IMMEDIATE` transaction that re-reads the
//! version first, so two processes opening a new database at once each run a
//! step once, and a failing step leaves the version and the schema as they
//! were. A step may also declare a WAL `TRUNCATE` checkpoint barrier
//! ([`Step::checkpoint_truncate_before`]) that runs outside any transaction
//! before its `IMMEDIATE` transaction begins. A database at a version past the
//! end of the list was migrated by a newer build, and is refused for reads and
//! writes alike.

mod run;
mod step;

pub use step::Step;

pub(crate) use run::{refuse_newer, refuse_past, run};
