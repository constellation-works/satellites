//! Reading the end of a file, never more than the caller allows.

use super::open::{Access, open_regular};
use super::read::Cap;
use crate::error::{Error, Result};
use crate::inspect::check_owner_and_mode;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::Path;

/// Read the last `cap` bytes of the regular file at `path`, or the whole
/// file when it is shorter.
///
/// For a log that grows without bound: a caller that needs only its newest
/// record reads the end, whatever the file's size. The bytes start wherever
/// the cap falls, so they can begin in the middle of a line (or of a UTF-8
/// character); a caller that wants whole lines drops everything up to the
/// first line break.
///
/// The file is opened as [`super::read`] opens it: a symbolic link, a FIFO,
/// a device or a directory is refused without blocking, and on Unix the
/// opened file must be owned by this user and open to no group or other
/// permission bit. At most `cap` bytes are read.
///
/// # Errors
///
/// [`Error::Symlink`] or [`Error::NotRegular`] for anything but a regular
/// file; [`Error::ForeignOwner`] or [`Error::Permissions`] for a file that
/// is not private; [`Error::Io`] when the file is absent (its source is
/// [`std::io::ErrorKind::NotFound`]) or cannot be read.
pub fn read_tail(path: &Path, cap: Cap) -> Result<Vec<u8>> {
    let mut file = open_regular(path, Access::Read, "read")?;
    let meta = file.metadata().map_err(|e| Error::io("inspect", path, e))?;
    check_owner_and_mode(path, &meta, "600")?;
    let start = meta.len().saturating_sub(cap.get());
    file.seek(SeekFrom::Start(start))
        .map_err(|e| Error::io("read", path, e))?;
    let mut bytes = Vec::new();
    file.take(cap.get())
        .read_to_end(&mut bytes)
        .map_err(|e| Error::io("read", path, e))?;
    Ok(bytes)
}
