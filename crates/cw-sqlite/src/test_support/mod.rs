//! For a project's own tests, behind the `test-support` feature: a database
//! in a temporary root, and a database staged at an older schema version.

use crate::db::Db;
use crate::error::{Error, Result};
use crate::migration::Step;
use std::path::Path;
use tempfile::TempDir;

/// The file name [`temp_db`] gives the database in its root.
pub const DB_FILE: &str = "test.db";

/// A [`Db`] named `test.db` in a new temporary root, created `0700`. Nothing is
/// created in the root until the first call; the root is removed when the
/// [`TempDir`] is dropped, so keep it for as long as the database is used.
///
/// # Errors
///
/// [`std::io::Error`] when the temporary root cannot be created.
pub fn temp_db(migrations: &'static [Step]) -> std::io::Result<(TempDir, Db)> {
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700));
    let root = builder.tempdir()?;
    let db = Db::new(root.path().join(DB_FILE), migrations);
    Ok((root, db))
}

/// Create the database at `path` at schema `version`: the first `version`
/// steps of `migrations`, and nothing written. A test then opens it with the
/// whole list to exercise the later steps alone.
///
/// # Errors
///
/// [`Error::Incompatible`] when `migrations` has fewer than `version`
/// steps, or any error of [`Db::write`].
pub fn stage(path: &Path, migrations: &'static [Step], version: u32) -> Result<()> {
    let steps = usize::try_from(version)
        .ok()
        .and_then(|count| migrations.get(..count))
        .ok_or_else(|| Error::Incompatible {
            path: path.to_path_buf(),
            found: version,
            supported: u32::try_from(migrations.len()).unwrap_or(u32::MAX),
        })?;
    Db::new(path, steps).write(|_| Ok::<_, Error>(()))
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "the test reads the mode of the temporary root it made, the one fact it checks"
    )]
    fn the_temporary_root_is_owner_only() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt as _;
        let (root, db) = super::temp_db(&[])?;
        let mode = std::fs::metadata(root.path())?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        assert_eq!(db.path().parent(), Some(root.path()));
        Ok(())
    }
}
