//! The child's process group: watched, signalled and reaped, with a drop
//! guard that sweeps it on a panic or an early return.

use rustix::io::Errno;
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid};
use std::io;
use std::process::{Child, ExitStatus};

/// A spawned child that leads its own process group. Until [`Group::reap`]
/// takes it, dropping the guard sweeps the group and reaps the leader.
pub(super) struct Group {
    child: Option<Child>,
    /// The kernel reaped the leader itself (the calling process ignores
    /// `SIGCHLD`): its id may be reused, so nothing is signalled any more.
    reaped_elsewhere: bool,
}

impl Group {
    pub(super) fn new(child: Child) -> Self {
        Self {
            child: Some(child),
            reaped_elsewhere: false,
        }
    }

    /// The leader, while it is held.
    pub(super) fn child_mut(&mut self) -> Option<&mut Child> {
        self.child.as_mut()
    }

    /// Whether the leader has exited, leaving it unreaped so the group's id
    /// stays this run's.
    pub(super) fn leader_exited(&mut self) -> bool {
        let Some(child) = &self.child else {
            return true;
        };
        let options = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
        match waitid(WaitId::Pid(Pid::from_child(child)), options) {
            Ok(Some(_)) => true,
            Ok(None) | Err(Errno::INTR) => false,
            Err(Errno::CHILD) => {
                self.reaped_elsewhere = true;
                true
            }
            // Any other failure is retried at the next poll; the deadline
            // still bounds the run.
            Err(_) => false,
        }
    }

    /// Signal the whole group. Never after the reap: the leader is held, and
    /// not reaped elsewhere.
    pub(super) fn signal(&self, signal: Signal) {
        if self.reaped_elsewhere {
            return;
        }
        if let Some(child) = &self.child {
            // The leader is unreaped, so the group exists; a failure here
            // leaves the reap below to end the leader.
            let _ = kill_process_group(Pid::from_child(child), signal);
        }
    }

    /// Sweep the group with `SIGKILL` and reap the leader. `None` when it was
    /// already reaped by an earlier call.
    pub(super) fn reap(&mut self) -> Option<io::Result<ExitStatus>> {
        self.signal(Signal::KILL);
        let mut child = self.child.take()?;
        Some(child.wait())
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        let _ = self.reap();
    }
}
