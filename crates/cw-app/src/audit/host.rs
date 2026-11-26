//! The identity the host establishes for an audit line: the effective user ID
//! of the process that writes it.
//!
//! A caller cannot set it. An environment variable, a client's label and a
//! request payload are claims; the process's user ID is not one of them. It
//! names the account the process runs as, so several clients run as one user
//! share it: it says which account wrote a line, not which client did.
//!
//! Unix only. Where the platform has no user ID (Windows) the line records
//! `euid` as `null`, a recorded gap rather than a claim.

/// The effective user ID of this process, stamped on each line at write time,
/// or `None` where the platform has none.
#[cfg(unix)]
pub(super) fn euid() -> Option<u32> {
    Some(rustix::process::geteuid().as_raw())
}

/// The effective user ID of this process, stamped on each line at write time,
/// or `None` where the platform has none.
#[cfg(not(unix))]
pub(super) fn euid() -> Option<u32> {
    None
}
