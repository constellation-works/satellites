//! Directories and inspection through the public API, on temporary roots.

use cw_fs::{create_private_dir, inspect_dir, inspect_file, list, write_atomic};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A temporary directory that is owner-only whatever the umask.
fn private_tempdir() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    create_private_dir(&dir.path().join("root"))?;
    Ok(dir)
}

#[test]
fn list_is_sorted_and_skips_finder_files() -> TestResult {
    let dir = private_tempdir()?;
    let root = dir.path().join("root");
    for name in ["b", ".DS_Store", "a", "._a", "c"] {
        write_atomic(&root.join(name), b"x")?;
    }
    assert_eq!(list(&root)?, ["a", "b", "c"]);
    Ok(())
}

#[test]
fn list_reads_an_absent_directory_as_empty_without_creating_it() -> TestResult {
    let dir = private_tempdir()?;
    let absent = dir.path().join("root").join("absent");
    assert!(list(&absent)?.is_empty());
    assert!(!absent.exists(), "listing created the directory");
    Ok(())
}

#[test]
fn inspect_returns_false_for_an_absent_path() -> TestResult {
    let dir = private_tempdir()?;
    let root = dir.path().join("root");
    assert!(!inspect_dir(&root.join("absent"))?);
    assert!(!inspect_file(&root.join("absent"))?);
    assert!(inspect_dir(&root)?);
    Ok(())
}

#[cfg(unix)]
mod unix {
    use super::{TestResult, private_tempdir};
    use cw_fs::{Error, inspect_dir, inspect_file, write_atomic};
    use std::fs::{Permissions, set_permissions};
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;

    /// Every group and other bit, alone.
    const LOOSE_BITS: [u32; 6] = [0o040, 0o020, 0o010, 0o004, 0o002, 0o001];

    fn assert_refused_with_fix(
        result: Result<bool, Error>,
        path: &Path,
        want_mode: u32,
        fix: &str,
    ) -> TestResult {
        let err = result.err().ok_or("loose state was accepted")?;
        let message = err.to_string();
        assert!(
            matches!(err, Error::Permissions { mode, .. } if mode == want_mode),
            "got {err:?}"
        );
        // The path is quoted, so the remedy is one shell word.
        let chmod = format!("chmod {fix} '{}'", path.display());
        assert!(message.contains(&chmod), "{message:?} lacks {chmod:?}");
        Ok(())
    }

    #[test]
    fn a_symlinked_directory_is_refused_even_when_it_leads_to_a_private_one() -> TestResult {
        use cw_fs::list;
        let dir = private_tempdir()?;
        let root = dir.path().join("root");
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&root, &link)?;
        let err = inspect_dir(&link)
            .err()
            .ok_or("a linked directory passed")?;
        assert!(matches!(err, Error::Symlink { .. }), "got {err:?}");
        let err = list(&link).err().ok_or("a linked directory was listed")?;
        assert!(matches!(err, Error::Symlink { .. }), "got {err:?}");
        let dangling = dir.path().join("dangling");
        std::os::unix::fs::symlink(dir.path().join("nowhere"), &dangling)?;
        let err = inspect_dir(&dangling)
            .err()
            .ok_or("a dangling link passed")?;
        assert!(matches!(err, Error::Symlink { .. }), "got {err:?}");
        Ok(())
    }

    #[test]
    fn inspect_file_refuses_any_group_or_other_bit_with_its_chmod() -> TestResult {
        let dir = private_tempdir()?;
        let path = dir.path().join("root").join("data");
        write_atomic(&path, b"{}")?;
        for bit in LOOSE_BITS {
            set_permissions(&path, Permissions::from_mode(0o600 | bit))?;
            assert_refused_with_fix(inspect_file(&path), &path, 0o600 | bit, "600")?;
        }
        set_permissions(&path, Permissions::from_mode(0o600))?;
        assert!(inspect_file(&path)?);
        Ok(())
    }

    #[test]
    fn inspect_dir_refuses_any_group_or_other_bit_with_its_chmod() -> TestResult {
        let dir = private_tempdir()?;
        let root = dir.path().join("root");
        for bit in LOOSE_BITS {
            set_permissions(&root, Permissions::from_mode(0o700 | bit))?;
            let result = inspect_dir(&root);
            set_permissions(&root, Permissions::from_mode(0o700))?;
            assert_refused_with_fix(result, &root, 0o700 | bit, "700")?;
        }
        assert!(inspect_dir(&root)?);
        Ok(())
    }
}
