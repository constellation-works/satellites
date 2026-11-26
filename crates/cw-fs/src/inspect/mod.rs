//! Checking existing state before it is trusted.
//!
//! Everything this crate creates is owner-only (`0700` and `0600`), so state
//! that is another user's, or open to any group or other permission bit, was
//! changed by someone else and is refused, with the `chmod` that fixes it in
//! the error. These checks fail closed. Once a directory is verified
//! owner-only, no other user can swap an entry in it between a check and the
//! open that follows; a process of the same user is not a boundary.
//!
//! On native Windows the owner and mode checks are skipped; a symbolic link
//! is still refused.

#![expect(
    clippy::disallowed_methods,
    reason = "inspect is the metadata adapter: it stats a path without following a link, which is how a link is refused"
)]

mod owner;

pub(crate) use owner::check_owner_and_mode;

use crate::error::{Error, Result};
use crate::file::open_regular_judge as judge;
use std::io;
use std::path::Path;

/// Check an existing private directory; `Ok(false)` when it does not exist.
///
/// A symbolic link at `dir` is refused, not followed, even when it leads to
/// a private directory or nowhere, and however `dir` is spelled (`link/`
/// included): whoever can replace the link can point the data anywhere. A link in a component above `dir` is used as given.
///
/// # Errors
///
/// [`Error::Symlink`] for a link; [`Error::ForeignOwner`] or
/// [`Error::Permissions`] when the directory is refused; [`Error::Io`] when
/// it cannot be inspected or is not a directory.
pub fn inspect_dir(dir: &Path) -> Result<bool> {
    // Without its trailing separators: `link/` makes the kernel resolve the
    // link before `lstat` could report it.
    let entry: std::path::PathBuf = dir.components().collect();
    let meta = match std::fs::symlink_metadata(&entry) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(Error::io("inspect", dir, e)),
    };
    if meta.file_type().is_symlink() {
        return Err(Error::Symlink {
            path: dir.to_path_buf(),
        });
    }
    if !meta.is_dir() {
        return Err(Error::io(
            "inspect",
            dir,
            io::Error::new(io::ErrorKind::NotADirectory, "expected a directory"),
        ));
    }
    check_owner_and_mode(dir, &meta, "700").map(|()| true)
}

/// Check an existing private file; `Ok(false)` when it does not exist. A
/// link is refused, not followed.
///
/// # Errors
///
/// [`Error::Symlink`], [`Error::NotRegular`], [`Error::ForeignOwner`] or
/// [`Error::Permissions`] when it is refused; [`Error::Io`] when it cannot
/// be inspected.
pub fn inspect_file(path: &Path) -> Result<bool> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(Error::io("inspect", path, e)),
    };
    judge(path, meta.file_type())?;
    check_owner_and_mode(path, &meta, "600").map(|()| true)
}
