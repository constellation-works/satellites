//! The private temporary root a test runs in.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Error;
use crate::child;

/// A private temporary directory for one test, removed on drop.
///
/// The directory is created owner-only (`0700` on Unix) under the system
/// temporary directory. It holds:
///
/// - `home/`: the `HOME` a child sees, created empty;
/// - `bin/`: first on the `PATH` a child sees, created empty, for fake
///   programs a test writes;
/// - `root/`: the application's data root (`<APP>_ROOT`), *not* created, so
///   the program under test meets it as a first run would.
///
/// The type is parameterised by the application's environment prefix:
/// `TempRoot::new("MYAPP")` points `MYAPP_ROOT` at `root/`.
#[derive(Debug)]
pub struct TempRoot {
    dir: tempfile::TempDir,
    root_var: String,
}

impl TempRoot {
    /// Create a temp root for the application whose variables start with
    /// `prefix` (`"MYAPP"` for `MYAPP_ROOT`).
    ///
    /// # Errors
    ///
    /// [`Error::InvalidPrefix`] if `prefix` is not a capital letter followed
    /// by capitals, digits or `_` (and not ending in `_`); [`Error::Io`] if a
    /// directory cannot be created.
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
        let made = Self {
            dir,
            root_var: format!("{prefix}_ROOT"),
        };
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

    /// The name of the root variable, `<APP>_ROOT`.
    #[must_use]
    pub fn root_var(&self) -> &str {
        &self.root_var
    }

    /// A path inside the temp root but outside the home, the `bin/` and the
    /// data root, for files a test needs somewhere neutral. Nothing is
    /// created.
    #[must_use]
    pub fn elsewhere(&self, name: impl AsRef<Path>) -> PathBuf {
        self.path().join("elsewhere").join(name)
    }

    /// A command for `program` with an isolated environment.
    ///
    /// The environment is cleared, then `HOME` is set to [`home`](Self::home),
    /// `<APP>_ROOT` to [`app_root`](Self::app_root), and `PATH` to
    /// [`bin`](Self::bin) followed by the system's standard program
    /// directories (`/usr/bin:/bin` on Unix). Nothing else is inherited: a
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
    })
}
