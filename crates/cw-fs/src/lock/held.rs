//! The per-thread record of the locks a thread holds.
//!
//! An advisory lock belongs to an open file, so a thread that opens a lock
//! file it already holds and asks again would wait on itself until its
//! deadline. This record lets [`super::Lock::acquire`] refuse at once.

use crate::error::{Error, Result};
use std::cell::RefCell;
use std::fs::File;
use std::path::Path;

/// Which lock file a lock is on, however the caller spelled its path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Key(Identity);

/// On Unix the file itself (device and inode); elsewhere its absolute path.
#[cfg(unix)]
type Identity = (u64, u64);
#[cfg(not(unix))]
type Identity = std::path::PathBuf;

impl Key {
    /// The key of the open lock file at `path`.
    #[cfg(unix)]
    pub(super) fn of(file: &File, path: &Path) -> Result<Self> {
        use std::os::unix::fs::MetadataExt as _;
        let meta = file.metadata().map_err(|e| Error::io("inspect", path, e))?;
        Ok(Self((meta.dev(), meta.ino())))
    }

    /// The key of the open lock file at `path`.
    #[cfg(not(unix))]
    pub(super) fn of(_file: &File, path: &Path) -> Result<Self> {
        std::path::absolute(path)
            .map(Self)
            .map_err(|e| Error::io("resolve", path, e))
    }
}

thread_local! {
    static HELD: RefCell<Vec<Key>> = const { RefCell::new(Vec::new()) };
}

/// Whether this thread holds the lock `key` names.
pub(super) fn contains(key: &Key) -> bool {
    HELD.with(|held| held.borrow().contains(key))
}

/// Record that this thread now holds `key`.
pub(super) fn insert(key: Key) {
    HELD.with(|held| held.borrow_mut().push(key));
}

/// Record that this thread released `key`.
pub(super) fn remove(key: &Key) {
    HELD.with(|held| {
        let mut held = held.borrow_mut();
        if let Some(at) = held.iter().position(|k| k == key) {
            held.swap_remove(at);
        }
    });
}
