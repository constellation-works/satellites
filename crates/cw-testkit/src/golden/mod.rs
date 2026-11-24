//! The golden harness: output compared with checked-in files.

mod timestamps;

use std::path::{Component, Path, PathBuf};

use crate::Error;

pub use timestamps::{TIMESTAMP_PLACEHOLDER, redact_timestamps};

/// The variable that rewrites goldens instead of comparing: set it to `1`
/// (`UPDATE_GOLDENS=1 cargo test`), then review the diff as a contract
/// change. Any other value, or none, compares.
pub const UPDATE_VAR: &str = "UPDATE_GOLDENS";

/// The golden files of one test target, in one folder.
///
/// [`Goldens::new`] reads [`UPDATE_VAR`] once. When it is `1`,
/// [`check`](Self::check) writes the output to the golden file and passes;
/// otherwise it compares and returns [`Error::GoldenMismatch`] or
/// [`Error::GoldenMissing`], whose message names the file and the update
/// variable.
///
/// Goldens conventionally live in the test's own `tests/goldens/` folder:
///
/// ```no_run
/// use cw_testkit::{Goldens, redact_timestamps};
///
/// let goldens = Goldens::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/goldens"));
/// let output = String::from("created 2026-01-02T03:04:05Z\n");
/// goldens.assert("list/plain.txt", &redact_timestamps(&output));
/// ```
#[derive(Debug, Clone)]
pub struct Goldens {
    dir: PathBuf,
    update: bool,
}

impl Goldens {
    /// The goldens under `dir`, updating if [`UPDATE_VAR`] is `1` in this
    /// process's environment.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            update: std::env::var_os(UPDATE_VAR).is_some_and(|v| v == "1"),
        }
    }

    /// The folder the goldens live in.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Whether [`check`](Self::check) rewrites files instead of comparing.
    #[must_use]
    pub fn is_updating(&self) -> bool {
        self.update
    }

    /// Compare `actual` with the golden file `name` (a relative path under
    /// [`dir`](Self::dir)), or write it there when updating.
    ///
    /// Returns an error instead of panicking, so a test can check many
    /// outputs and report every stale golden at once.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidGoldenName`] if `name` is empty, absolute or contains
    /// `..`; [`Error::GoldenSymlink`] if [`dir`](Self::dir) itself, the
    /// golden file or a directory between them is a symbolic link, when
    /// comparing and when updating; [`Error::GoldenMissing`] or [`Error::GoldenMismatch`] when
    /// comparing; [`Error::Io`] if the file cannot be read or written.
    pub fn check(&self, name: impl AsRef<Path>, actual: &str) -> Result<(), Error> {
        // `shown` is the path as configured, for messages; `resolved` is the
        // one path every inspection and access goes through.
        let shown = self.path_of(name.as_ref())?;
        let resolved = self.resolve(name.as_ref())?;
        if self.update {
            return write(&resolved, actual);
        }
        let path = shown;
        let expected = match std::fs::read_to_string(&resolved) {
            Ok(expected) => expected,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::GoldenMissing {
                    path,
                    actual: actual.to_owned(),
                });
            }
            Err(source) => {
                return Err(Error::Io {
                    action: "read the golden",
                    path,
                    source,
                });
            }
        };
        if expected == actual {
            Ok(())
        } else {
            Err(Error::GoldenMismatch {
                path,
                expected,
                actual: actual.to_owned(),
            })
        }
    }

    /// [`check`](Self::check), panicking with the error's message on any
    /// failure.
    ///
    /// # Panics
    ///
    /// Whenever [`check`](Self::check) returns an error.
    #[track_caller]
    pub fn assert(&self, name: impl AsRef<Path>, actual: &str) {
        if let Err(err) = self.check(name, actual) {
            panic!("{err}");
        }
    }

    /// The golden file's path, resolved once: [`dir`](Self::dir) made
    /// canonical (links above it are the caller's and are followed), then
    /// `name` below it, refusing a symbolic link at the folder itself and at
    /// every existing step down to the file. `name` is already checked to
    /// hold only normal components. Once a step does not exist, nothing
    /// below it does either, so the rest is joined unchecked.
    fn resolve(&self, name: &Path) -> Result<PathBuf, Error> {
        let mut path = match lstat(&self.dir)? {
            Some(meta) if meta.file_type().is_symlink() => {
                return Err(Error::GoldenSymlink {
                    path: self.dir.clone(),
                });
            }
            Some(_) => std::fs::canonicalize(&self.dir).map_err(|source| Error::Io {
                action: "resolve the goldens folder",
                path: self.dir.clone(),
                source,
            })?,
            None => return Ok(self.dir.join(name)),
        };
        let mut exists = true;
        for part in name.components() {
            path.push(part);
            if !exists {
                continue;
            }
            match lstat(&path)? {
                Some(meta) if meta.file_type().is_symlink() => {
                    return Err(Error::GoldenSymlink { path });
                }
                Some(_) => {}
                None => exists = false,
            }
        }
        Ok(path)
    }

    fn path_of(&self, name: &Path) -> Result<PathBuf, Error> {
        let inside = name.components().next().is_some()
            && name
                .components()
                .all(|part| matches!(part, Component::Normal(_)));
        if inside {
            Ok(self.dir.join(name))
        } else {
            Err(Error::InvalidGoldenName {
                name: name.to_path_buf(),
            })
        }
    }
}

/// `path`'s own metadata, not following a link, or `None` if it does not
/// exist.
fn lstat(path: &Path) -> Result<Option<std::fs::Metadata>, Error> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Io {
            action: "inspect the golden path",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn write(path: &Path, actual: &str) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::Io {
            action: "create the goldens folder",
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(path, actual).map_err(|source| Error::Io {
        action: "write the golden",
        path: path.to_path_buf(),
        source,
    })
}
