//! The log file: appended under its own lock, read within a cap.

use super::line::{Record, TailScope, decode, encode, newest_format};
use crate::error::Error;
use crate::error::Result;
use cw_fs::{Cap, Lock};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The audit log's name under the data directory.
pub const FILE: &str = "audit.jsonl";

/// The audit log's lock file under the data directory. It is never a
/// program's own lock file.
pub const LOCK_FILE: &str = ".audit.lock";

/// How much of the log's end an append reads to find its newest line.
const TAIL: u64 = 64 * 1024;

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
    /// Under the lock it reads the log's last 64 KiB first. A log whose
    /// newest line is in a format newer than this build writes is refused,
    /// so an older build never mixes its lines into a newer log; so is a log
    /// whose newest line cannot be found in that tail. A torn last line is
    /// closed with a line break before the record, so the record stays whole.
    ///
    /// # Errors
    ///
    /// [`crate::Error::NewerLog`] or [`crate::Error::UnknownLog`] when the
    /// log is refused as above;
    /// [`crate::Error::Fs`] when the directory or the log is refused or a
    /// step fails, including the lock still held at the deadline;
    /// [`crate::Error::Encode`] when the line cannot be encoded.
    pub fn append(&self, record: &Record, wait: Duration) -> Result<()> {
        let line = encode(record)?;
        if !cw_fs::inspect_dir(&self.root)? {
            cw_fs::create_private_dir(&self.root)?;
        }
        let _lock = Lock::acquire(&self.lock_path(), "audit append", wait)?;
        let path = self.path();
        let mut bytes = Vec::with_capacity(line.len() + 1);
        if cw_fs::inspect_file(&path)? {
            // One byte past the window tells a file of exactly TAIL bytes,
            // read whole, from the cut end of a longer one.
            let tail = cw_fs::read_tail(&path, Cap::bytes(TAIL + 1))?;
            let whole_file = u64::try_from(tail.len()).unwrap_or(u64::MAX) <= TAIL;
            check_newest(&path, &tail, whole_file)?;
            // A torn last line (a crash or short write mid-append) is closed,
            // so this record starts a line of its own instead of being
            // joined to it.
            if tail.last().is_some_and(|&b| b != b'\n') {
                bytes.push(b'\n');
            }
        }
        bytes.extend_from_slice(&line);
        cw_fs::append(&path, &bytes)?;
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

/// Refuse to append to the log at `path` whose last bytes are `tail` when
/// its newest line with a readable format is in a newer format, or when no
/// such line is in a tail cut from a longer file.
fn check_newest(path: &std::path::Path, tail: &[u8], whole_file: bool) -> Result<()> {
    match newest_format(tail, TailScope::of(whole_file)) {
        Ok(Some(found)) if found > super::FORMAT => Err(Error::NewerLog {
            path: path.to_path_buf(),
            found,
            supported: super::FORMAT,
        }),
        Ok(_) => Ok(()),
        Err(()) => Err(Error::UnknownLog {
            path: path.to_path_buf(),
            tail: TAIL,
        }),
    }
}
