//! One exclusive advisory lock, with a deadline.
//!
//! The lock is the operating system's advisory lock (`flock` on Unix,
//! `LockFileEx` on Windows, through [`std::fs::File::try_lock`]) on a file
//! that is created once and never deleted. The kernel releases it when the
//! holder dies, so a crash cannot wedge the data the way a create-if-absent
//! lock file would; deleting the file would let a third process lock a fresh
//! file at the same path while a second still holds the old one. Rust opens
//! files close-on-exec, so a child process never inherits it.
//!
//! - **Every acquire opens the file again**, and the lock belongs to that
//!   open file, so a second thread of one process waits like another
//!   process.
//! - **The same thread acquiring a lock it holds is refused at once**
//!   ([`crate::Error::Reentrant`]), from a per-thread record of the locks it
//!   holds, rather than waiting out its own deadline.
//! - **A deadline is required.** The wait polls, starting at 1 ms and
//!   doubling to 20 ms, so a lock released quickly is taken quickly.
//! - **The holder is recorded** in the lock file as one JSON line (pid,
//!   time, label) after acquiring, and cleared on release. A waiter that
//!   times out names the holder only while the recorded process is alive;
//!   nothing decides ownership from the record.
//! - **The lock file is opened without following a link** and refused when
//!   it has more than one hard link (on Unix), so the lock cannot be pointed
//!   at, or written as, another file.

mod held;
mod holder;

pub use holder::Holder;

use crate::error::{Error, Result};
use crate::file::{Access, open_regular};
use std::fs::{File, TryLockError};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The first pause between attempts while the lock is held elsewhere.
const FIRST_POLL: Duration = Duration::from_millis(1);

/// The longest pause between attempts.
const LONGEST_POLL: Duration = Duration::from_millis(20);

/// A held exclusive lock. Released when dropped.
///
/// Not `Send`: the record of held locks that refuses re-entry is per
/// thread, so the lock is released on the thread that took it. A thread
/// that needs the lock takes its own.
#[must_use = "the lock is released as soon as it is dropped"]
#[derive(Debug)]
pub struct Lock {
    file: File,
    path: PathBuf,
    key: held::Key,
    not_send: PhantomData<*const ()>,
}

impl Lock {
    /// Take the exclusive lock on the file at `path`, creating it `0600` when
    /// absent, and waiting up to `wait` while another holder has it.
    ///
    /// `label` says what the holder is doing; it is recorded with the pid and
    /// the time so a waiter that times out can say who is in the way. A
    /// `wait` of zero tries once.
    ///
    /// # Errors
    ///
    /// - [`Error::Locked`] when the lock is still held at the deadline.
    /// - [`Error::Reentrant`] at once when this thread already holds it.
    /// - [`Error::Symlink`] or [`Error::NotRegular`] when the lock file is a
    ///   link, not a regular file, or has a second hard link.
    /// - [`Error::Io`] when the file cannot be opened, locked or written.
    pub fn acquire(path: &Path, label: &str, wait: Duration) -> Result<Self> {
        let file = open_regular(path, Access::Lock, "open lock file")?;
        check_single_link(&file, path)?;
        let key = held::Key::of(&file, path)?;
        if held::contains(&key) {
            return Err(Error::Reentrant {
                path: path.to_path_buf(),
            });
        }
        take(&file, path, wait)?;
        // A link may have appeared while this call waited: check again
        // before writing the record. Dropping the file releases the lock.
        check_single_link(&file, path)?;
        holder::record(&file, label).map_err(|e| Error::io("record the holder of", path, e))?;
        held::insert(key.clone());
        Ok(Self {
            file,
            path: path.to_path_buf(),
            key,
            not_send: PhantomData,
        })
    }

    /// The lock file this lock holds.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // Never fail or panic in a destructor. Clearing the record only keeps
        // the next waiter from reading a stale one, and closing the file
        // releases the lock even if the explicit unlock fails.
        let _ = holder::clear(&self.file);
        let _ = self.file.unlock();
        held::remove(&self.key);
    }
}

/// Poll for the lock until it is taken or `wait` has passed.
fn take(file: &File, path: &Path, wait: Duration) -> Result<()> {
    let started = Instant::now();
    let mut pause = FIRST_POLL;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(()),
            Err(TryLockError::WouldBlock) => {
                let waited = started.elapsed();
                let Some(left) = wait.checked_sub(waited).filter(|left| !left.is_zero()) else {
                    return Err(Error::Locked {
                        path: path.to_path_buf(),
                        holder: holder::read_live(file),
                        waited,
                    });
                };
                // An in-process sleep, never a forked `sleep`.
                std::thread::sleep(pause.min(left));
                pause = pause.saturating_mul(2).min(LONGEST_POLL);
            }
            Err(TryLockError::Error(e)) => return Err(Error::io("lock", path, e)),
        }
    }
}

/// Refuse a lock file reachable under another name: whoever holds that name
/// could write the lock file without opening it here.
#[cfg(unix)]
fn check_single_link(file: &File, path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    let meta = file.metadata().map_err(|e| Error::io("inspect", path, e))?;
    match meta.nlink() {
        1 => Ok(()),
        links => Err(Error::NotRegular {
            path: path.to_path_buf(),
            found: crate::error::EntryKind::HardLinked { links },
        }),
    }
}

/// No link count to check on this platform.
#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the Unix twin can fail, and the caller treats both alike"
)]
fn check_single_link(_file: &File, _path: &Path) -> Result<()> {
    Ok(())
}
