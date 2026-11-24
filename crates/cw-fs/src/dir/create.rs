//! Creating a private directory.

use crate::error::{Error, Result};
use std::path::Path;

/// Create `dir` and every missing parent, each new one `0700`. A directory
/// that already exists is left as its owner made it.
///
/// # Errors
///
/// [`Error::Io`] when a component cannot be created.
pub fn create_private_dir(dir: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(dir)
        .map_err(|e| Error::io("create directory", dir, e))
}
