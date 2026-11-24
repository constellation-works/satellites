//! Writing a whole file durably: replacing one, or creating one that must
//! not replace another.

use crate::dir::sync_parent;
use crate::error::{Error, Result};
use std::io::Write as _;
use std::path::Path;
use tempfile::NamedTempFile;

/// Replace `path` with `bytes`, atomically and durably. The file is `0600`.
///
/// The bytes go to a temporary file in the same directory, which is flushed
/// and renamed over `path`; then the directory is flushed. A reader sees the
/// whole old content or the whole new content, never a mix. On Linux and
/// macOS a crash after this returns cannot bring the old content back; on
/// native Windows the directory flush is a no-op, so that is as the volume
/// keeps the rename. A symbolic link at
/// `path` is replaced by the file; its target is never written. The
/// temporary file is removed on every failure.
///
/// # Errors
///
/// [`Error::Io`] when any step fails, the directory flush included (a
/// filesystem that cannot flush directories at all is tolerated). On native
/// Windows the directory flush is a no-op.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let staged = stage(path, bytes)?;
    staged
        .persist(path)
        .map_err(|e| Error::io("replace", path, e.error))?;
    sync_parent(path)
}

/// Create `path` holding `bytes`, durably and `0600`, where nothing of that
/// name exists yet. It never replaces anything, a symbolic link included.
///
/// # Errors
///
/// [`Error::Io`] when the name is taken (its source is
/// [`std::io::ErrorKind::AlreadyExists`]) or any step fails.
pub fn create_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let staged = stage(path, bytes)?;
    staged
        .persist_noclobber(path)
        .map_err(|e| Error::io("create", path, e.error))?;
    sync_parent(path)
}

/// A flushed temporary file holding `bytes`, beside `path` so the rename
/// cannot cross filesystems. `tempfile` creates it `0600` and with
/// `O_EXCL`, and deletes it on drop unless it is persisted.
fn stage(path: &Path, bytes: &[u8]) -> Result<NamedTempFile> {
    let parent = crate::dir::parent_of(path);
    let mut staged = NamedTempFile::new_in(parent)
        .map_err(|e| Error::io("create a temporary file in", parent, e))?;
    staged
        .write_all(bytes)
        .and_then(|()| staged.as_file().sync_all())
        .map_err(|e| Error::io("write", staged.path().to_path_buf(), e))?;
    Ok(staged)
}
