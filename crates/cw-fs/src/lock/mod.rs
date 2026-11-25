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
//!   holds, rather than waiting out its own deadline. On Unix the record
//!   names the file (device and inode), so any spelling of its path is
//!   caught; on native Windows it names the absolute path as spelled, so
//!   another spelling of the same file (`STATE.lock` for `state.lock`)
//!   waits out the deadline and fails with [`crate::Error::Locked`].
//! - **A deadline is required.** The wait polls, starting at 1 ms and
//!   doubling to 20 ms, so a lock released quickly is taken quickly. No
//!   attempt is started after the deadline. The deadline bounds the
//!   waiting, not the instant the lock is taken: an attempt that starts just
//!   before it may finish a few microseconds after.
//! - **The holder is recorded** in the lock file as one JSON line (pid,
//!   time, label) after acquiring, and cleared on release. A waiter that
//!   times out names the holder only while the recorded process is alive,
//!   as far as the platform can tell (see [`Holder`]); nothing decides
//!   ownership from the record. On native Windows a timeout names no
//!   holder, this process included.
//! - **The record fails open.** It only explains a timeout to the next
//!   waiter, so a lock whose record cannot be written or synced is still
//!   acquired; the failure is kept on the [`Lock`] for the caller to warn
//!   about ([`Lock::holder_error`]), and a waiter that times out meanwhile
//!   names no holder.
//! - **The lock file must be private.** On every platform a symbolic link at
//!   its path is refused. On Unix it must also be owned by this user, open
//!   to no group or other permission bit, and have one hard link, so the
//!   lock cannot be pointed at, or written as, another file; native Windows
//!   skips those three checks.

mod held;
mod holder;

pub use holder::Holder;

use crate::error::{Error, Result};
use crate::file::{Access, open_regular};
use crate::inspect::check_owner_and_mode;
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
    holder_error: Option<Error>,
    not_send: PhantomData<*const ()>,
}

impl Lock {
    /// Take the exclusive lock on the file at `path`, creating it `0600` when
    /// absent, and waiting up to `wait` while another holder has it.
    ///
    /// `label` says what the holder is doing; it is recorded with the pid and
    /// the time so a waiter that times out can say who is in the way. The
    /// whole record is at most 4096 bytes, so a longer label is recorded cut
    /// at a character boundary. A `wait` of zero tries once; no attempt is
    /// started after `wait` has passed, though one started just before may
    /// finish a few microseconds after.
    ///
    /// # Errors
    ///
    /// - [`Error::Locked`] when the lock is still held at the deadline.
    /// - [`Error::Reentrant`] at once when this thread already holds it.
    /// - [`Error::Symlink`] or [`Error::NotRegular`] when the lock file is a
    ///   link, not a regular file, or has a second hard link.
    /// - [`Error::ForeignOwner`] or [`Error::Permissions`] when the lock file
    ///   is another user's or open to any group or other permission bit.
    /// - [`Error::Io`] when the file cannot be opened, inspected or locked.
    ///
    /// Writing the holder record is not among them: once the lock is taken,
    /// a record that cannot be written or synced leaves the lock acquired,
    /// and the failure is returned by [`Lock::holder_error`].
    pub fn acquire(path: &Path, label: &str, wait: Duration) -> Result<Self> {
        let file = open_regular(path, Access::Lock, "open lock file")?;
        let meta = file.metadata().map_err(|e| Error::io("inspect", path, e))?;
        check_owner_and_mode(path, &meta, "600")?;
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
        // Fail open: the record is diagnostic, and the lock is already held.
        let holder_error = holder::record(&file, label)
            .err()
            .map(|e| Error::io("record the holder of", path, e));
        held::insert(key.clone());
        Ok(Self {
            file,
            path: path.to_path_buf(),
            key,
            holder_error,
            not_send: PhantomData,
        })
    }

    /// Why the holder record could not be written when this lock was taken,
    /// or `None` when it was. Diagnostic only: the lock is held either way.
    /// A caller may warn with it; nothing should decide from it. While it is
    /// `Some`, a waiter that times out names no holder.
    #[must_use]
    pub fn holder_error(&self) -> Option<&Error> {
        self.holder_error.as_ref()
    }

    /// The lock file this lock holds.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

// `Lock` was unwind safe before it kept `holder_error`, and stays so. The
// stored error is the only field that is not: an `io::Error` may carry a
// boxed `dyn Error` with no `RefUnwindSafe` bound. That is sound here. The
// error is set once in `acquire` and never mutated, and `Lock` hands it out
// only by shared reference, so a panic caught mid-use cannot leave it, or the
// lock, in a broken state that code after the `catch_unwind` could observe.
impl std::panic::UnwindSafe for Lock {}
impl std::panic::RefUnwindSafe for Lock {}

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
///
/// The first attempt is made at once, so a zero `wait` tries exactly once.
/// Every later attempt is made only while the deadline has not passed: a
/// sleep can end late, and a lock released while this thread slept must not
/// be taken after the caller's deadline.
fn take(file: &File, path: &Path, wait: Duration) -> Result<()> {
    let started = Instant::now();
    let mut pause = FIRST_POLL;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(()),
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(e)) => return Err(Error::io("lock", path, e)),
        }
        let Some(left) = remaining(started, wait) else {
            return Err(timed_out(file, path, started));
        };
        // An in-process sleep, never a forked `sleep`.
        std::thread::sleep(pause.min(left));
        pause = pause.saturating_mul(2).min(LONGEST_POLL);
        if remaining(started, wait).is_none() {
            return Err(timed_out(file, path, started));
        }
    }
}

/// The time left before the deadline, or `None` once it has passed.
fn remaining(started: Instant, wait: Duration) -> Option<Duration> {
    wait.checked_sub(started.elapsed())
        .filter(|left| !left.is_zero())
}

fn timed_out(file: &File, path: &Path, started: Instant) -> Error {
    Error::Locked {
        path: path.to_path_buf(),
        holder: holder::read_live(file),
        waited: started.elapsed(),
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
