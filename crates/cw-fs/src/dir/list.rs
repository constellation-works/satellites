//! Listing a private directory.

use crate::error::{Error, Result};
use crate::inspect::inspect_dir;
use std::path::Path;

/// The names in `dir`, sorted. A directory that does not exist yet lists as
/// empty, and listing never creates it.
///
/// Finder's files (`.DS_Store` and `._*`) are left out: macOS writes them
/// into any directory a person browses, and they are never anyone's state.
/// A name that is not UTF-8 is left out too, since no caller here wrote it.
///
/// # Errors
///
/// [`Error::ForeignOwner`] or [`Error::Permissions`] when the directory is
/// refused; [`Error::Io`] when it cannot be read.
pub fn list(dir: &Path) -> Result<Vec<String>> {
    if !inspect_dir(dir)? {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| Error::io("list", dir, e))? {
        let entry = entry.map_err(|e| Error::io("list", dir, e))?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if !is_finder_file(&name) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

fn is_finder_file(name: &str) -> bool {
    name == ".DS_Store" || name.starts_with("._")
}
