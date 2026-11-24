//! The database file and its WAL-mode companions.

use crate::error::{Error, Result};
use std::io::{self, Read as _};
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
/// A path that is not valid UTF-8 is refused, because a reader hands SQLite
/// a `file:` URI, which is text.
pub(crate) fn resolve(path: &Path) -> Result<PathBuf> {
    if path.to_str().is_none() {
        return Err(Error::File(cw_fs::Error::Io {
            action: "open",
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "the path is not valid UTF-8, which a SQLite URI requires",
            ),
        }));
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

/// What the files beside an existing database say, looked at without SQLite.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sidecars {
    /// The `-wal` file exists and is not empty: it may hold committed pages
    /// the main file does not.
    pub(crate) wal_has_content: bool,
    /// The `-shm` index exists.
    pub(crate) shm: bool,
}

/// Look at the `-wal` and `-shm` beside the database at `path`.
pub(crate) fn sidecars(path: &Path) -> Result<Sidecars> {
    let len = |suffix: &str| {
        let file = sidecar(path, suffix);
        match std::fs::symlink_metadata(&file) {
            Ok(meta) => Ok(Some(meta.len())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(Error::File(cw_fs::Error::Io {
                action: "inspect",
                path: file,
                source,
            })),
        }
    };
    Ok(Sidecars {
        wal_has_content: len("-wal")?.is_some_and(|len| len > 0),
        shm: len("-shm")?.is_some(),
    })
}

/// What the main file's 100-byte header says.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Header {
    /// The database is in WAL mode (read and write format versions both 2).
    pub(crate) wal: bool,
    /// `PRAGMA user_version`, as the main file holds it.
    pub(crate) user_version: u32,
}

/// The main file's header, read without SQLite; `None` while the file is
/// shorter than a header (a database created and not yet written) or does
/// not start like a SQLite database. Only authoritative when the `-wal` has
/// no content.
pub(crate) fn header(path: &Path) -> Result<Option<Header>> {
    let io_error = |source| {
        Error::File(cw_fs::Error::Io {
            action: "read the header of",
            path: path.to_path_buf(),
            source,
        })
    };
    let mut bytes = [0_u8; 100];
    let mut file = std::fs::File::open(path).map_err(io_error)?;
    match file.read_exact(&mut bytes) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(io_error(e)),
    }
    if !bytes.starts_with(b"SQLite format 3\0") {
        // Not a database: SQLite's own open says so, as `Corrupt`.
        return Ok(None);
    }
    let [a, b, c, d] = [bytes[60], bytes[61], bytes[62], bytes[63]];
    Ok(Some(Header {
        wal: bytes[18] == 2 && bytes[19] == 2,
        user_version: u32::from_be_bytes([a, b, c, d]),
    }))
}
