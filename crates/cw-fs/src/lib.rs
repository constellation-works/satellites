//! File access for a program's private data directory: durable writes,
//! appends, bounded reads, owner-only checks, and one advisory lock with a
//! deadline.
//!
//! Mechanism only. It knows no record and no file name: a caller hands it a
//! path and bytes, and wraps its [`Error`] in its own typed error with its
//! own code.
//!
//! - [`file`](mod@file): [`write_atomic`], [`create_new`], [`append`], [`read`] with a
//!   [`Cap`], [`remove`].
//! - [`dir`]: [`list`], [`create_private_dir`].
//! - [`inspect`]: [`inspect_file`], [`inspect_dir`], which refuse state that
//!   is a link, another user's, or open to any group or other permission bit.
//! - [`lock`]: [`Lock`], an exclusive OS advisory lock on a file that is
//!   never deleted.
//! - [`error`]: the typed [`Error`] all of them fail with.
//!
//! Everything is created owner-only (`0700` directories, `0600` files). On a
//! platform without Unix modes (Windows) the owner, mode and link checks are
//! skipped; writes and locks still work.

pub mod dir;
pub mod error;
pub mod file;
pub mod inspect;
pub mod lock;

pub use dir::{create_private_dir, list};
pub use error::{EntryKind, Error, Result};
pub use file::{Cap, append, create_new, read, remove, write_atomic};
pub use inspect::{inspect_dir, inspect_file};
pub use lock::{Holder, Lock};
