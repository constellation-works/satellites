//! Flushing a directory so a name created, renamed or removed in it
//! survives a crash.

#![cfg_attr(
    unix,
    expect(
        clippy::disallowed_types,
        reason = "the directory flush opens the directory read only to sync it; this module is the directory adapter's only handle"
    )
)]

use crate::error::{Error, Result};
use std::path::Path;

/// The directory that holds `path`: `.` for a bare file name.
pub(crate) fn parent_of(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

/// Flush the directory that holds `path`.
pub(crate) fn sync_parent(path: &Path) -> Result<()> {
    let dir = parent_of(path);
    sync_dir(dir).map_err(|e| Error::io("sync directory", dir, e))
}

/// A filesystem that cannot flush a directory at all answers the flush with
/// `EINVAL` or `ENOTSUP` and has no stronger guarantee to give; the rename
/// already happened, so that is not a failed write. Any other failure is,
/// and so is any failure to open the directory.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> std::io::Result<()> {
    use rustix::io::Errno;
    let handle = std::fs::File::open(dir)?;
    match handle.sync_all() {
        Ok(()) => Ok(()),
        Err(e) if matches!(Errno::from_io_error(&e), Some(Errno::INVAL | Errno::NOTSUP)) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Windows cannot open a directory as a file to flush it, and NTFS journals
/// the rename itself.
#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the Unix twin can fail, and callers treat both alike"
)]
fn sync_dir(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}
