//! The private temporary root a test runs in.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Error;
use crate::child;

/// A private temporary directory for one test, removed on drop.
///
/// The directory is created owner-only (`0700` on Unix) under the system
/// temporary directory, and so are `home/` and `bin/`: the mode is set
/// explicitly after each directory is made, so a restrictive umask cannot
/// leave one narrower than `0700`. It holds:
///
/// - `home/`: the `HOME` a child sees, created empty;
/// - `bin/`: first on the `PATH` a child sees, created empty, for fake
///   programs a test writes;
/// - `root/`: the application's data root (`<APP>_ROOT`), *not* created, so
///   the program under test meets it as a first run would.
///
/// The type is parameterised by the application's environment prefix:
/// `TempRoot::new("MYAPP")` points `MYAPP_ROOT` at `root/`.
///
/// Dropping the root removes it best-effort: a failure (a read-only
/// directory a test left inside, say) is ignored and the tree stays behind.
/// Call [`close`](Self::close) to remove it and see the error.
#[derive(Debug)]
pub struct TempRoot {
    dir: tempfile::TempDir,
    root_var: String,
    /// The canonical parent of the temp root: git looks for a repository no
    /// higher than the root itself.
    git_ceiling: PathBuf,
}

impl TempRoot {
    /// Create a temp root for the application whose variables start with
    /// `prefix` (`"MYAPP"` for `MYAPP_ROOT`).
    ///
    /// # Errors
    ///
    /// [`Error::InvalidPrefix`] if `prefix` is not a capital letter followed
    /// by capitals, digits or `_` (and not ending in `_`);
    /// [`Error::PathSeparatorInRoot`] if the `bin/` path contains the
    /// platform's `PATH` separator (`:` on Unix, `;` on Windows), which would
    /// split a child's `PATH` (the system temporary directory, from `TMPDIR`
    /// on Unix, decides that path); [`Error::Io`] if a directory cannot be
    /// created or given its mode.
    pub fn new(prefix: &str) -> Result<Self, Error> {
        if !valid_prefix(prefix) {
            return Err(Error::InvalidPrefix {
                prefix: prefix.to_owned(),
            });
        }
        let dir = private_dir_builder()
            .prefix("cw-testkit.")
            .tempdir()
            .map_err(|source| Error::Io {
                action: "create the temp root",
                path: std::env::temp_dir(),
                source,
            })?;
        set_private(dir.path())?;
        // Canonical, because git compares its ceiling with the canonical
        // working directory (the system temporary directory may sit behind
        // a link, as /var does on macOS).
        let canonical = std::fs::canonicalize(dir.path()).map_err(|source| Error::Io {
            action: "resolve the temp root",
            path: dir.path().to_path_buf(),
            source,
        })?;
        let git_ceiling = canonical
            .parent()
            .map_or_else(|| canonical.clone(), Path::to_path_buf);
        let made = Self {
            dir,
            root_var: format!("{prefix}_ROOT"),
            git_ceiling,
        };
        let bin = made.bin();
        // Lossy is exact here: the separator is ASCII, and no replacement
        // character is one.
        if bin.to_string_lossy().contains(PATH_SEPARATOR) {
            return Err(Error::PathSeparatorInRoot { path: bin });
        }
        for sub in [made.home(), made.bin()] {
            create_private_dir(&sub)?;
        }
        Ok(made)
    }

    /// The temp root itself.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// The `HOME` a child sees: `<temp root>/home`.
    #[must_use]
    pub fn home(&self) -> PathBuf {
        self.path().join("home")
    }

    /// The directory first on a child's `PATH`: `<temp root>/bin`.
    #[must_use]
    pub fn bin(&self) -> PathBuf {
        self.path().join("bin")
    }

    /// The application's data root a child sees as `<APP>_ROOT`:
    /// `<temp root>/root`. It does not exist until something creates it.
    #[must_use]
    pub fn app_root(&self) -> PathBuf {
        self.path().join("root")
    }

    /// The directory git may not climb into from the temp root.
    pub(crate) fn git_ceiling(&self) -> &Path {
        &self.git_ceiling
    }

    /// The name of the root variable, `<APP>_ROOT`.
    #[must_use]
    pub fn root_var(&self) -> &str {
        &self.root_var
    }

    /// A path inside the temp root but outside the home, the `bin/` and the
    /// data root, for files a test needs somewhere neutral. Nothing is
    /// created.
    ///
    /// `name` must be a relative path without `..`: it is joined as given,
    /// so an absolute path or enough `..` components lead outside the temp
    /// root.
    #[must_use]
    pub fn elsewhere(&self, name: impl AsRef<Path>) -> PathBuf {
        self.path().join("elsewhere").join(name)
    }

    /// Remove the temp root and everything in it, returning what went wrong
    /// instead of ignoring it as drop does.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the tree cannot be removed entirely; what could not
    /// be removed stays at the error's path.
    pub fn close(self) -> Result<(), Error> {
        let path = self.path().to_path_buf();
        self.dir.close().map_err(|source| Error::Io {
            action: "remove the temp root",
            path,
            source,
        })
    }

    /// A command for `program` with an isolated environment, run in the temp
    /// root.
    ///
    /// The working directory is [`path`](Self::path), so a child starts
    /// inside the fixture, not in the checkout the tests run from; the test
    /// may set another with [`Command::current_dir`]. That alone does not
    /// stop a child looking upward: the temp root sits under the system
    /// temporary directory, which may itself be inside a repository. So
    /// `GIT_CEILING_DIRECTORIES` is set to the temp root's parent, and git
    /// run in the temp root or below never discovers a repository above it.
    /// Two limits: a tool that searches upward for its own files by other
    /// means is not bounded, and on Unix git splits the variable at `:`, so
    /// if the temp root's parent has a `:` in its path the ceiling is lost
    /// and git may discover an enclosing repository. The environment is cleared, then `HOME` is set to [`home`](Self::home),
    /// `<APP>_ROOT` to [`app_root`](Self::app_root), and `PATH` to
    /// [`bin`](Self::bin) followed by the system's standard program
    /// directories (`/usr/bin:/bin` on Unix), and `GIT_CEILING_DIRECTORIES`
    /// as above. Nothing else is inherited: a
    /// variable the test needs is set on the returned command with
    /// [`Command::env`].
    ///
    /// `program` is resolved as [`Command::new`] resolves it; give an
    /// absolute path (such as `env!("CARGO_BIN_EXE_<name>")`) to run the
    /// exact binary cargo built.
    #[must_use]
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        child::isolated(program.as_ref(), self)
    }
}

/// What separates `PATH` entries.
pub(crate) const PATH_SEPARATOR: &str = if cfg!(windows) { ";" } else { ":" };

/// `[A-Z][A-Z0-9_]*`, not ending in `_`.
fn valid_prefix(prefix: &str) -> bool {
    let mut bytes = prefix.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_uppercase())
        && bytes.all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && !prefix.ends_with('_')
}

fn private_dir_builder() -> tempfile::Builder<'static, 'static> {
    #[cfg_attr(
        not(unix),
        allow(unused_mut, reason = "permissions are only set on Unix")
    )]
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    builder
}

fn create_private_dir(path: &Path) -> Result<(), Error> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(|source| Error::Io {
        action: "create",
        path: path.to_path_buf(),
        source,
    })?;
    set_private(path)
}

/// Set `path` to `0700` on Unix. The mode given at creation goes through the
/// umask, so it alone can leave a directory narrower (`0500` under
/// `umask 0277`, which also stops the kit creating anything inside).
#[cfg_attr(
    not(unix),
    allow(clippy::unnecessary_wraps, reason = "modes are Unix-only")
)]
fn set_private(path: &Path) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(
            |source| Error::Io {
                action: "set the mode of",
                path: path.to_path_buf(),
                source,
            },
        )?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
