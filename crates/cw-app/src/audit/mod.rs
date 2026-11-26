//! The append-only audit log in `<root>/audit.jsonl`.
//!
//! It answers "who changed this data, with what command, when, and did it
//! work". [`crate::exec()`] appends a started line before running a command
//! an [`crate::App`] declares mutating. The surface settles the [`Pending`]
//! outcome after rendering, appending it under the same ID.
//!
//! - **Identifiers only.** A line carries the command name, the record it
//!   acted on, the outcome, the stable error code, the surface and the
//!   actor: never an argument value or an error message, which can hold the
//!   data itself or a path. The actor is what the client claimed, so it is
//!   written only when it reads as an identifier ([`ACTOR_PLACEHOLDER`]
//!   otherwise).
//! - **An explicit unknown outcome.** The reader pairs starts and outcomes
//!   by ID. A start without an outcome reads as [`Status::Unknown`].
//! - **Its own lock.** Lines are appended through [`cw_fs::append`] under
//!   `<root>/.audit.lock`, never a program's own lock, so a command that
//!   timed out waiting for the program's lock still records its failure.
//! - **The start is required.** If its append fails, `exec` returns the
//!   error without running the command. Once the command has run, an
//!   outcome append error reaches the surface as an [`AuditWarning`].
//!   This crate prints and logs nothing.
//! - **Versioned.** Each line carries its `format`; the reader upgrades an
//!   older line through declared steps and refuses a newer one.
//!   An append reads the log's newest line first and refuses a log in a
//!   newer format (or one whose newest line it cannot find), so an older
//!   build never mixes its lines into a newer log. A torn last line is
//!   closed before the next record, so that record stays whole.

mod actor;
mod line;
mod log;
mod meta;
mod pending;

pub use actor::{ACTOR_MAX_LEN, ACTOR_PLACEHOLDER};
pub use line::{FORMAT, Record, Status};
pub use log::{AuditLog, FILE, LOCK_FILE};
pub use meta::AuditMeta;
pub use pending::{AuditWarning, Pending};
