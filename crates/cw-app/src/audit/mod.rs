//! The append-only audit log in `<root>/audit.jsonl`.
//!
//! It answers "who changed this data, with what command, when, and did it
//! work". [`crate::exec()`] appends a started line before running a command
//! an [`crate::App`] declares mutating. The surface settles the [`Pending`]
//! outcome after rendering, appending it under the same ID.
//!
//! - **Identifiers only.** A line carries the command name, the record it
//!   acted on or was aimed at, the outcome, the stable error code, the
//!   surface and the actor: never an argument value or an error message,
//!   which can hold the data itself or a path. The actor is what the client
//!   claimed and the target is what the app supplied, so each is written
//!   only when it reads as an identifier ([`ACTOR_PLACEHOLDER`] and
//!   [`TARGET_PLACEHOLDER`] otherwise).
//! - **The target is known before the command runs.** An app may resolve the
//!   id of the record a command is aimed at ([`crate::App::subject`]); the
//!   started line carries it, and the outcome line keeps it on failure, so
//!   a refused write and a start with no outcome still name their record.
//! - **An explicit unknown outcome.** The reader pairs starts and outcomes
//!   by ID. A start without an outcome reads as [`Status::Unknown`].
//! - **Its own lock.** Lines are appended through [`cw_fs::append`] under
//!   `<root>/.audit.lock`, never a program's own lock, so a command that
//!   timed out waiting for the program's lock still records its failure.
//! - **The start is required.** If its append fails, `exec` returns the
//!   error without running the command. Once the command has run, an
//!   outcome append error reaches the surface as an [`AuditWarning`].
//!   This crate prints and logs nothing.
//! - **Itemized commands.** A command declared with [`AuditMeta::itemized`]
//!   writes zero or many records. `exec` writes nothing before it runs; the
//!   command calls [`AuditHandle::admit`] (from [`crate::Ctx::audit`]) inside
//!   its transaction, after its last write and before the commit, and every
//!   item's started line goes out in one append under one lock hold. A failed
//!   admit is `audit_start_failed`: the command returns it and rolls back. The
//!   surface settles the [`Batch`] after the reply: each item's outcome is
//!   what was admitted, or a failure with the command's code when `run`
//!   returned an error. A command that fails having admitted nothing leaves
//!   one standalone failure line; one that succeeds having admitted nothing
//!   leaves none. Each item is an ordinary pair under its own id, so the
//!   format and [`AuditLog::read`] are the same for both forms.
//! - **Versioned.** Each line carries its `format`; the reader upgrades an
//!   older line through declared steps and refuses a newer one.
//!   An append reads the log's newest line first and refuses a log in a
//!   newer format (or one whose newest line it cannot find), so an older
//!   build never mixes its lines into a newer log. A torn last line is
//!   closed before the next record, so that record stays whole.

mod actor;
mod identifier;
mod itemized;
mod line;
mod log;
mod meta;
mod pending;
mod target;

pub use actor::{ACTOR_MAX_LEN, ACTOR_PLACEHOLDER};
pub(crate) use itemized::Live;
pub use itemized::{AuditHandle, Batch, Item};
pub use line::{FORMAT, Record, Status};
pub use log::{AuditLog, FILE, LOCK_FILE};
pub use meta::AuditMeta;
pub use pending::{AuditWarning, Pending};
pub use target::{TARGET_MAX_LEN, TARGET_PLACEHOLDER};
