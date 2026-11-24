//! The database file and its WAL-mode companions.

use crate::error::Result;
use std::io;
use std::path::{Path, PathBuf};

/// The `-wal` and `-shm` files beside the database at `path`.
fn sidecars(path: &Path) -> [PathBuf; 2] {
    let side = |suffix: &str| {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        PathBuf::from(name)
    };
    [side("-wal"), side("-shm")]
}

/// Refuse the database or a companion that is a link, another user's, or
/// open to a group or other permission bit; `Ok(false)` when the database
/// does not exist.
pub(crate) fn inspect(path: &Path) -> Result<bool> {
    for sidecar in sidecars(path) {
        cw_fs::inspect_file(&sidecar)?;
    }
    Ok(cw_fs::inspect_file(path)?)
}

/// Inspect the database, and create it empty and `0600` when it is missing.
/// Another process creating it first is not an error: the file it made is
/// inspected like any other.
pub(crate) fn create_if_missing(path: &Path) -> Result<()> {
    if inspect(path)? {
        return Ok(());
    }
    match cw_fs::create_new(path, &[]) {
        Ok(()) => Ok(()),
        Err(cw_fs::Error::Io { source, .. }) if source.kind() == io::ErrorKind::AlreadyExists => {
            inspect(path).map(|_| ())
        }
        Err(e) => Err(e.into()),
    }
}

/// `path` with its directory resolved to the real one, for SQLite's open.
///
/// The open refuses a symbolic link anywhere in the name it is given
/// (`SQLITE_OPEN_NOFOLLOW`). The database itself was inspected already and a
/// link there refused; a directory reached through a link above it (`/var`
/// on macOS, a data directory a person named through one) is used as given,
/// as `cw-fs` uses it.
pub(crate) fn resolve(path: &Path) -> Result<PathBuf> {
    let (Some(name), Some(parent)) = (path.file_name(), path.parent()) else {
        return Ok(path.to_path_buf());
    };
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let real = std::fs::canonicalize(parent).map_err(|source| cw_fs::Error::Io {
        action: "resolve the directory of",
        path: path.to_path_buf(),
        source,
    })?;
    Ok(real.join(name))
}
