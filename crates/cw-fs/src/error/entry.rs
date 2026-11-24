//! What an entry turned out to be when a regular file was required.

use std::fmt;
use std::fs::FileType;

/// What is at a path where a regular file was required, so a refusal says
/// what is in the way rather than an errno. A symbolic link has its own
/// [`super::Error::Symlink`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// A directory.
    Directory,
    /// A named pipe: a read would wait for a writer that may never come.
    Fifo,
    /// A Unix domain socket.
    Socket,
    /// A block or character device, which a read might never finish.
    Device,
    /// A regular file reachable under more than one name. A lock file must
    /// have one: another name for it is a way to write it unseen.
    HardLinked {
        /// How many names it has.
        links: u64,
    },
    /// Something else the platform has.
    Other,
}

impl EntryKind {
    /// What `file_type` is, or `None` when it is a regular file or a link
    /// (each judged elsewhere).
    pub(crate) fn of(file_type: FileType) -> Option<Self> {
        if file_type.is_file() || file_type.is_symlink() {
            return None;
        }
        if file_type.is_dir() {
            return Some(Self::Directory);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt as _;
            if file_type.is_fifo() {
                return Some(Self::Fifo);
            }
            if file_type.is_socket() {
                return Some(Self::Socket);
            }
            if file_type.is_block_device() || file_type.is_char_device() {
                return Some(Self::Device);
            }
        }
        Some(Self::Other)
    }
}

impl fmt::Display for EntryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Directory => f.write_str("a directory"),
            Self::Fifo => f.write_str("a FIFO"),
            Self::Socket => f.write_str("a socket"),
            Self::Device => f.write_str("a device"),
            Self::HardLinked { links } => write!(f, "a file with {links} hard links"),
            Self::Other => f.write_str("a special file"),
        }
    }
}
