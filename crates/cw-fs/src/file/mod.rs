//! Files: replaced whole, created once, appended to, read within a cap,
//! removed.
//!
//! - **Durable.** [`write_atomic`] and [`create_new`] write a temporary file
//!   in the same directory, flush it, rename it into place and flush the
//!   directory, so a crash leaves the old file or the new one, never a torn
//!   one, and (where the directory is flushed) not the old one once the call
//!   has returned. A filesystem that cannot flush a directory at all (it answers
//!   `EINVAL` or `ENOTSUP`) is tolerated; any other failure is an error. On
//!   native Windows the directory flush is a no-op: a directory cannot be
//!   opened there to flush it, so the rename is as durable as the volume
//!   makes it (NTFS journals it).
//! - **Owner-only.** Every file is created `0600`.
//! - **A file opened by name is a regular file.** [`read`] and [`append`]
//!   judge the entry with `lstat`, open it with `O_NOFOLLOW` and
//!   `O_NONBLOCK`, and judge the descriptor again, so a symbolic link, a
//!   FIFO, a device or a directory is refused, and nothing blocks on one.
//!   On native Windows the `lstat` judgement still refuses a symbolic link
//!   and a directory, but the open itself carries neither flag.

mod append;
mod open;
mod read;
mod remove;
mod tail;
mod write;

pub use append::append;
pub(crate) use open::{Access, judge as open_regular_judge, open_regular};
pub use read::{Cap, read};
pub use remove::remove;
pub use tail::read_tail;
pub use write::{create_new, write_atomic};
