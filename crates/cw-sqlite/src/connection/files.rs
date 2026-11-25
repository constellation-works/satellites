//! The database file and its WAL-mode companions.

use crate::error::{Error, Result};
use std::io;
use std::path::{Path, PathBuf};

/// The `suffix` file (`-wal`, `-shm`) beside the database at `path`.
pub(crate) fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// `path` with its directory resolved to the real one: what is inspected,
/// created and handed to SQLite, so all three name the same file.
///
/// SQLite's open refuses a symbolic link anywhere in the name it is given
/// (`SQLITE_OPEN_NOFOLLOW`), and the database itself is inspected and a link
/// there refused; a directory reached through a link above it (`/var` on
/// macOS, a data directory a person named through one) is used as given, as
/// `cw-fs` uses it. The parent directory is trusted: a process that can
/// swap it can swap the database too.
///
/// A path that is not valid UTF-8, as given or once resolved, is refused,
/// because a reader hands SQLite a `file:` URI, which is text.
pub(crate) fn resolve(path: &Path) -> Result<PathBuf> {
    if path.to_str().is_none() {
        return Err(not_utf8(path));
    }
    let (Some(name), Some(parent)) = (path.file_name(), path.parent()) else {
        return Ok(path.to_path_buf());
    };
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    match std::fs::canonicalize(parent) {
        // The resolved directory can differ from the spelling checked above.
        Ok(real) if real.to_str().is_none() => Err(not_utf8(path)),
        Ok(real) => Ok(real.join(name)),
        // No directory, so no database: the caller's inspect says so.
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(source) => Err(Error::File(cw_fs::Error::Io {
            action: "resolve the directory of",
            path: path.to_path_buf(),
            source,
        })),
    }
}

/// The refusal of a path that is not valid UTF-8.
fn not_utf8(path: &Path) -> Error {
    Error::File(cw_fs::Error::Io {
        action: "open",
        path: path.to_path_buf(),
        source: io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path is not valid UTF-8, which a SQLite URI requires",
        ),
    })
}

/// Refuse the database or a companion that is a link, another user's, or
/// open to a group or other permission bit; `Ok(false)` when the database
/// does not exist.
pub(crate) fn inspect(path: &Path) -> Result<bool> {
    for suffix in ["-wal", "-shm"] {
        cw_fs::inspect_file(&sidecar(path, suffix))?;
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

/// Whether the `-wal` beside the database at `path` exists and is not
/// empty, so it may hold committed pages the main file does not. Looked at
/// by name, without opening it.
pub(crate) fn wal_has_content(path: &Path) -> Result<bool> {
    let wal = sidecar(path, "-wal");
    match std::fs::symlink_metadata(&wal) {
        Ok(meta) => Ok(meta.len() > 0),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(Error::File(cw_fs::Error::Io {
            action: "inspect",
            path: wal,
            source,
        })),
    }
}

/// Whether this process may create files in the database's directory. When
/// it may not and the index cannot be read, no writer can exist there
/// either (it would need to create the `-wal` or `-shm`), so the main file
/// is static: a copy, or a database on a read-only mount.
pub(crate) fn directory_writable(path: &Path) -> Result<bool> {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        Some(_) | None => Path::new("."),
    };
    writable(dir).map_err(|source| {
        Error::File(cw_fs::Error::Io {
            action: "check whether it can write beside",
            path: path.to_path_buf(),
            source,
        })
    })
}

/// Whether this process can write the `-shm` index of the database at
/// `path`: the `-shm` itself when it exists, or else the directory, where
/// SQLite would create it.
pub(crate) fn index_writable(path: &Path) -> Result<bool> {
    let shm = sidecar(path, "-shm");
    let exists = match std::fs::symlink_metadata(&shm) {
        Ok(_) => true,
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(source) => return Err(check_failed(&shm, source)),
    };
    if !exists {
        return directory_writable(path);
    }
    writable(&shm).map_err(|source| check_failed(&shm, source))
}

fn check_failed(path: &Path, source: io::Error) -> Error {
    Error::File(cw_fs::Error::Io {
        action: "check whether it can write",
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(unix)]
fn writable(path: &Path) -> io::Result<bool> {
    match rustix::fs::access(path, rustix::fs::Access::WRITE_OK) {
        Ok(()) => Ok(true),
        Err(e) if e == rustix::io::Errno::ACCESS || e == rustix::io::Errno::ROFS => Ok(false),
        Err(e) => Err(e.into()),
    }
}

#[cfg(not(unix))]
fn writable(path: &Path) -> io::Result<bool> {
    Ok(!std::fs::metadata(path)?.permissions().readonly())
}
