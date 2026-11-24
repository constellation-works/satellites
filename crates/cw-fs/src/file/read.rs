//! Reading a whole file, never more than the caller allows.

use super::open::{Access, open_regular};
use crate::error::{Error, Result};
use crate::inspect::check_owner_and_mode;
use std::io::Read as _;
use std::path::Path;

/// The most bytes a [`read`] accepts. Every read states one, so a file that
/// grew without bound cannot exhaust memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cap(u64);

impl Cap {
    /// A cap of `bytes` bytes.
    #[must_use]
    pub const fn bytes(bytes: u64) -> Self {
        Self(bytes)
    }

    /// The cap in bytes.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Read the whole regular file at `path`, provided it holds at most `cap`
/// bytes.
///
/// A symbolic link, a FIFO, a device or a directory at `path` is refused
/// without blocking and nothing is read from it. On Unix the opened file
/// must also be owned by this user and open to no group or other permission
/// bit, as [`crate::inspect_file`] requires. At most `cap` bytes plus one
/// are read, however large the file is.
///
/// # Errors
///
/// [`Error::Symlink`] or [`Error::NotRegular`] for anything but a regular
/// file; [`Error::ForeignOwner`] or [`Error::Permissions`] for a file that
/// is not private; [`Error::TooLarge`] past the cap; [`Error::Io`] when the file is
/// absent (its source is [`std::io::ErrorKind::NotFound`]) or cannot be
/// read.
pub fn read(path: &Path, cap: Cap) -> Result<Vec<u8>> {
    let file = open_regular(path, Access::Read, "read")?;
    let meta = file.metadata().map_err(|e| Error::io("inspect", path, e))?;
    check_owner_and_mode(path, &meta, "600")?;
    let mut bytes = Vec::new();
    file.take(cap.get().saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| Error::io("read", path, e))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > cap.get() {
        return Err(Error::TooLarge {
            path: path.to_path_buf(),
            cap: cap.get(),
        });
    }
    Ok(bytes)
}
