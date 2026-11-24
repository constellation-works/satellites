//! The log file: appended under its own lock, read within a cap.

use super::line::{Record, decode, encode};
use crate::error::Result;
use cw_fs::{Cap, Lock};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The audit log's name under the data directory.
pub const FILE: &str = "audit.jsonl";

/// The audit log's lock file under the data directory. It is never a
/// program's own lock file.
pub const LOCK_FILE: &str = ".audit.lock";

/// The audit log of one data directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditLog {
    root: PathBuf,
}

impl AuditLog {
    /// The log in the data directory `root`. Nothing is touched until a line
    /// is appended or read.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The data directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `<root>/audit.jsonl`.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.root.join(FILE)
    }

    /// `<root>/.audit.lock`.
    #[must_use]
    pub fn lock_path(&self) -> PathBuf {
        self.root.join(LOCK_FILE)
    }

    /// Append `record` as one line in the newest format, creating the data
    /// directory `0700` and the log `0600` when absent, and holding the
    /// audit lock, waiting up to `wait` for it, so lines never interleave.
    ///
    /// # Errors
    ///
    /// [`crate::Error::Fs`] when the directory or the log is refused or a
    /// step fails, including the lock still held at the deadline;
    /// [`crate::Error::Encode`] when the line cannot be encoded.
    pub fn append(&self, record: &Record, wait: Duration) -> Result<()> {
        let line = encode(record)?;
        if !cw_fs::inspect_dir(&self.root)? {
            cw_fs::create_private_dir(&self.root)?;
        }
        let _lock = Lock::acquire(&self.lock_path(), "audit append", wait)?;
        cw_fs::append(&self.path(), &line)?;
        Ok(())
    }

    /// Every line of the log, oldest first, provided the file holds at most
    /// `cap` bytes. An absent log has no lines.
    ///
    /// Each line is read on its own: a line that is not valid, or newer than
    /// this build reads, is returned as its error in its place while the
    /// rest are read. A last line without its newline was torn by a crash
    /// during its append and is left out; blank lines are skipped. Reading
    /// takes no lock: an append is one write, so a reader sees a line whole
    /// or not at all.
    ///
    /// # Errors
    ///
    /// [`crate::Error::Fs`] when the log is refused, larger than `cap`, or
    /// cannot be read. Per line, [`crate::Error::Corrupt`] or
    /// [`crate::Error::NewerFormat`].
    pub fn read(&self, cap: Cap) -> Result<Vec<Result<Record>>> {
        let path = self.path();
        if !cw_fs::inspect_file(&path)? {
            return Ok(Vec::new());
        }
        let bytes = cw_fs::read(&path, cap)?;
        let whole = match bytes.iter().rposition(|&b| b == b'\n') {
            Some(end) => &bytes[..=end],
            None => &[][..],
        };
        Ok(whole
            .split(|&b| b == b'\n')
            .enumerate()
            .filter(|(_, line)| !line.iter().all(u8::is_ascii_whitespace))
            .map(|(index, line)| decode(&path, index + 1, line))
            .collect())
    }
}
