//! The log file: appended under its own lock, read within a cap.

use super::line::{DecodedLine, Phase, Record, Status, TailScope, decode, encode, newest_format};
use crate::error::Error;
use crate::error::Result;
use cw_fs::{Cap, Lock};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
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

    /// Append `record` as one standalone line in the newest format. This
    /// creates the data directory `0700` and the log `0600` when absent and
    /// waits up to `wait` for the audit lock so lines never interleave.
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
        self.append_phase(record, &Phase::Standalone, wait)
    }

    pub(super) fn append_phase(
        &self,
        record: &Record,
        phase: &Phase,
        wait: Duration,
    ) -> Result<()> {
        let line = encode(record, phase)?;
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

    /// Audited commands in start order, provided the file holds at most
    /// `cap` bytes. An absent log has no records. Each start is paired with
    /// its outcome by ID. A start without an outcome has [`Status::Unknown`].
    ///
    /// A malformed or unsupported line, a second start with the same
    /// pending ID, or an outcome without a matching start is returned as
    /// a line error while the rest are read. The reader skips blank lines
    /// and an unterminated last line. Reading takes no lock because an
    /// append writes the line and its terminator together.
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
        let mut records = Vec::new();
        let mut starts = HashMap::new();
        for (index, line) in whole.split(|&b| b == b'\n').enumerate() {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let number = index + 1;
            match decode(&path, number, line) {
                Ok(DecodedLine {
                    phase: Phase::Standalone,
                    record,
                }) => records.push(Ok(record)),
                Ok(DecodedLine {
                    phase: Phase::Started { id },
                    mut record,
                }) => match starts.entry(id) {
                    Entry::Vacant(start) => {
                        start.insert(records.len());
                        record.status = Status::Unknown;
                        record.target = None;
                        record.error_code = None;
                        record.duration_ms = None;
                        records.push(Ok(record));
                    }
                    Entry::Occupied(_) => {
                        records.push(Err(invalid_pair(&path, number, "duplicate start ID")));
                    }
                },
                Ok(DecodedLine {
                    phase: Phase::Outcome { id },
                    record,
                }) => {
                    if let Some(start) = starts.remove(&id) {
                        records[start] = Ok(record);
                    } else {
                        records.push(Err(invalid_pair(
                            &path,
                            number,
                            "outcome has no matching start",
                        )));
                    }
                }
                Err(error) => records.push(Err(error)),
            }
        }
        Ok(records)
    }
}

fn invalid_pair(path: &Path, line: usize, message: &str) -> Error {
    Error::Corrupt {
        path: path.to_path_buf(),
        line,
        source: serde::de::Error::custom(message),
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
