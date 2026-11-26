//! Appending one record to a log.

#![expect(
    clippy::disallowed_types,
    reason = "append is the file adapter: it opens the log with owner only mode and holds the handle it writes to"
)]

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
/// The whole record goes to one `write` call on an `O_APPEND` descriptor; a
/// caller that holds a [`crate::Lock`] keeps records from interleaving. The
/// operating system may accept fewer bytes than asked (a full disk, a
/// quota, a signal): that short write is returned as an error rather than
/// finished with a second call, and it leaves the accepted prefix at the end
/// of the file. A reader of the log must treat a final line without its
/// terminator as torn. A file this call creates has its
/// directory flushed too, so the new name survives a crash.
///
/// An existing file is opened only when it is a regular file, owned by this
/// user and open to no other: a link is refused, not followed.
///
/// # Errors
///
/// [`Error::Symlink`], [`Error::NotRegular`], [`Error::ForeignOwner`] or
/// [`Error::Permissions`] for an existing file that is refused;
/// [`Error::Io`] when a step fails, a short write included (its source is
/// [`std::io::ErrorKind::WriteZero`]).
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
    let written = file
        .write(line)
        .map_err(|e| Error::io("append to", path, e))?;
    if written != line.len() {
        return Err(Error::io(
            "append to",
            path,
            io::Error::new(
                io::ErrorKind::WriteZero,
                format!("short write: {written} of {} bytes", line.len()),
            ),
        ));
    }
    file.sync_data().map_err(|e| Error::io("sync", path, e))?;
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
