//! Files: replaced whole, created once, appended to, read within a cap,
//! removed.
//!
//! - **Durable.** [`write_atomic`] and [`create_new`] write a temporary file
//!   in the same directory, flush it, rename it into place and flush the
//!   directory, so a crash leaves the old file or the new one, never a torn
//!   one. A filesystem that cannot flush a directory at all (it answers
//!   `EINVAL` or `ENOTSUP`) is tolerated; any other failure is an error.
//! - **Owner-only.** Every file is created `0600`.
//! - **A file opened by name is a regular file.** [`read`] and [`append`]
//!   judge the entry with `lstat`, open it with `O_NOFOLLOW` and
//!   `O_NONBLOCK`, and judge the descriptor again, so a symbolic link, a
//!   FIFO, a device or a directory is refused, and nothing blocks on one.

mod append;
mod open;
mod read;
mod remove;
mod write;

pub use append::append;
pub(crate) use open::{Access, judge as open_regular_judge, open_regular};
pub use read::{Cap, read};
pub use remove::remove;
pub use write::{create_new, write_atomic};
