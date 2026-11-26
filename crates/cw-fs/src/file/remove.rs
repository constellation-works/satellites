//! Removing a file durably.

#![expect(
    clippy::disallowed_methods,
    reason = "remove is the file adapter delete: it inspects the path and unlinks only a regular file it has judged"
)]

use super::open::judge;
use crate::dir::sync_parent;
use crate::error::{Error, Result};
use crate::inspect::{check_owner_and_mode, inspect_dir};
use std::io;
use std::path::Path;

/// Remove the regular file at `path` and flush its directory, so the removal
/// survives a crash as a write would. A file that is already gone, or whose
/// directory is, satisfies the call.
///
/// Never pass a lock file: a lock file is never deleted, or a third process
/// could lock a fresh file at the same path while a second still holds the
/// old one.
///
/// # Errors
///
/// [`Error::Symlink`], [`Error::NotRegular`], [`Error::ForeignOwner`] or
/// [`Error::Permissions`] when the file or its directory is refused;
/// [`Error::Io`] when a step fails.
pub fn remove(path: &Path) -> Result<()> {
    if !inspect_dir(crate::dir::parent_of(path))? {
        return Ok(());
    }
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::io("inspect", path, e)),
    };
    judge(path, meta.file_type())?;
    check_owner_and_mode(path, &meta, "600")?;
    match std::fs::remove_file(path) {
        Ok(()) => sync_parent(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io("remove", path, e)),
    }
}
