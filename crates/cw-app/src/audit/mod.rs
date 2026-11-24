//! The audit log: one line per mutating command, in `<root>/audit.jsonl`.
//!
//! It answers "who changed this data, with what command, when, and did it
//! work". [`crate::exec()`] starts a [`Pending`] record before a command an
//! [`crate::App`] declares mutating, marks it with the outcome, and hands it
//! to the surface, which settles it after rendering.
//!
//! - **Identifiers only.** A line carries the command name, the record it
//!   acted on, the outcome, the stable error code, the surface and the
//!   actor: never an argument value or an error message, which can hold the
//!   data itself or a path. The actor is what the client claimed, so it is
//!   written only when it reads as an identifier ([`ACTOR_PLACEHOLDER`]
//!   otherwise).
//! - **A failure until told otherwise.** A record starts as a failure, so a
//!   command that panics is logged as one; an outcome is never assumed.
//! - **Its own lock.** Lines are appended through [`cw_fs::append`] under
//!   `<root>/.audit.lock`, never a program's own lock, so a command that
//!   timed out waiting for the program's lock still records its failure.
//! - **A side channel: it fails open.** A line that cannot be written never
//!   fails the command that already ran; it comes back to the surface as an
//!   [`AuditWarning`] value to print. This crate prints and logs nothing.
//! - **Versioned.** Each line carries its `format`; the reader upgrades an
//!   older line through declared steps and refuses a newer one.

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
