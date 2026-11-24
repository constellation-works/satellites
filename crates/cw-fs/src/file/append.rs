//! Appending one record to a log.

use super::open::{Access, open_regular};
use crate::dir::sync_parent;
use crate::error::{Error, Result};
use crate::inspect::check_owner_and_mode;
use std::fs::File;
use std::io::{self, Write as _};
use std::path::Path;

/// Append `line` to `path` in one write and flush it, creating the file
/// `0600` when it is absent.
///
/// The whole record goes to a single write on an append-only descriptor, so
/// a crash can lose it but never tear it; a caller that holds a [`crate::Lock`]
/// keeps records from interleaving. A file this call creates has its
/// directory flushed too, so the new name survives a crash.
///
/// An existing file is opened only when it is a regular file, owned by this
/// user and open to no other: a link is refused, not followed.
///
/// # Errors
///
/// [`Error::Symlink`], [`Error::NotRegular`], [`Error::ForeignOwner`] or
/// [`Error::Permissions`] for an existing file that is refused;
/// [`Error::Io`] when a step fails.
pub fn append(path: &Path, line: &[u8]) -> Result<()> {
    let (mut file, created) = match create(path) {
        Ok(file) => (file, true),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let file = open_regular(path, Access::Append, "open")?;
            let meta = file.metadata().map_err(|e| Error::io("inspect", path, e))?;
            check_owner_and_mode(path, &meta, "600")?;
            (file, false)
        }
        Err(e) => return Err(Error::io("create", path, e)),
    };
    file.write_all(line)
        .and_then(|()| file.sync_data())
        .map_err(|e| Error::io("append to", path, e))?;
    if created { sync_parent(path) } else { Ok(()) }
}

/// Create `path` `0600` for appending, refusing a name that is taken (a
/// link included) with `AlreadyExists`.
fn create(path: &Path) -> io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.append(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}
