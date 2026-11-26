//! Opening a file by name only when it is a regular file.

#![expect(
    clippy::disallowed_types,
    reason = "open is the file adapter: it inspects a path and opens the one regular file the helpers return"
)]
#![expect(
    clippy::disallowed_methods,
    reason = "open is the file adapter: it inspects a path with symlink_metadata before it opens anything"
)]

use crate::error::{EntryKind, Error, Result};
use std::fs::{File, FileType};
use std::io;
use std::path::Path;

/// What a file is opened for.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Access {
    /// Read an existing file.
    Read,
    /// Append to an existing file.
    Append,
    /// Read and write, creating the file `0600` when it is absent: a lock.
    Lock,
}

/// Open `path` for `access`, provided it is a regular file.
///
/// The entry is judged three times, and each closes a gap the others leave:
///
/// - **Before the open**, with `lstat`, so a link, a device or a FIFO is
///   refused without being opened: opening one can block or have effects of
///   its own.
/// - **By the open**, which carries `O_NOFOLLOW` and `O_NONBLOCK` on Unix, so
///   an entry swapped in after that look cannot redirect it: the kernel
///   refuses a link, and a FIFO without a writer cannot hold the open.
/// - **On the descriptor**, which is what every later read, write or lock
///   uses.
///
/// Only the last component is judged: a directory reached through a link
/// above it is used as given.
pub(crate) fn open_regular(path: &Path, access: Access, action: &'static str) -> Result<File> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => judge(path, meta.file_type())?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::io("inspect", path, e)),
    }
    let file = open_flags(path, access).map_err(|e| {
        // Report what is there now rather than the errno, when it is
        // something this call refuses.
        std::fs::symlink_metadata(path)
            .ok()
            .and_then(|meta| judge(path, meta.file_type()).err())
            .unwrap_or_else(|| Error::io(action, path, e))
    })?;
    let meta = file.metadata().map_err(|e| Error::io("inspect", path, e))?;
    judge(path, meta.file_type())?;
    Ok(file)
}

/// Refuse `path` unless `file_type` is a regular file.
pub(crate) fn judge(path: &Path, file_type: FileType) -> Result<()> {
    if file_type.is_symlink() {
        return Err(Error::Symlink {
            path: path.to_path_buf(),
        });
    }
    match EntryKind::of(file_type) {
        None => Ok(()),
        Some(found) => Err(Error::NotRegular {
            path: path.to_path_buf(),
            found,
        }),
    }
}

#[cfg(unix)]
fn open_flags(path: &Path, access: Access) -> io::Result<File> {
    use rustix::fs::{Mode, OFlags};
    let flags = match access {
        Access::Read => OFlags::RDONLY,
        Access::Append => OFlags::WRONLY | OFlags::APPEND,
        Access::Lock => OFlags::RDWR | OFlags::CREATE,
    };
    let fd = rustix::fs::open(
        path,
        flags | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )?;
    Ok(File::from(fd))
}

#[cfg(not(unix))]
fn open_flags(path: &Path, access: Access) -> io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    match access {
        Access::Read => options.read(true),
        Access::Append => options.append(true),
        Access::Lock => options.read(true).write(true).create(true).truncate(false),
    };
    options.open(path)
}
