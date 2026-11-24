//! The owner and mode check.

use crate::error::Result;
use std::fs::Metadata;
use std::path::Path;

/// Refuse state that another user owns or that any group or other
/// permission bit reaches. `fix` is the owner-only mode to name in the
/// error: `700` for a directory, `600` for a file.
#[cfg(unix)]
pub(crate) fn check_owner_and_mode(path: &Path, meta: &Metadata, fix: &'static str) -> Result<()> {
    use crate::error::Error;
    use std::os::unix::fs::MetadataExt as _;
    let uid = rustix::process::geteuid().as_raw();
    if meta.uid() != uid {
        return Err(Error::ForeignOwner {
            path: path.to_path_buf(),
            owner: meta.uid(),
            uid,
        });
    }
    let mode = meta.mode() & 0o7777;
    if mode & 0o077 != 0 {
        return Err(Error::Permissions {
            path: path.to_path_buf(),
            mode,
            fix,
        });
    }
    Ok(())
}

/// No Unix owner or modes on this platform: nothing to check.
#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the Unix twin can fail, and callers treat both alike"
)]
pub(crate) fn check_owner_and_mode(
    _path: &Path,
    _meta: &Metadata,
    _fix: &'static str,
) -> Result<()> {
    Ok(())
}
