//! The child's process group: watched, signalled and reaped, with a drop
//! guard that sweeps it on a panic or an early return.

use rustix::io::Errno;
use rustix::process::{
    Pid, Signal, WaitId, WaitIdOptions, kill_process, kill_process_group, waitid,
};
use std::io;
use std::process::{Child, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

/// The first wait between two looks at the leader; it doubles to [`POLL_MAX`].
pub(super) const POLL_MIN: Duration = Duration::from_millis(1);

/// The longest wait between two looks at the leader.
pub(super) const POLL_MAX: Duration = Duration::from_millis(20);

/// How long the leader gets to exit after the final `SIGKILL` before it is
/// left unreaped rather than waited for without bound.
const REAP_WAIT: Duration = Duration::from_secs(2);

/// A spawned child that leads its own process group. Until [`Group::reap`]
/// takes it, dropping the guard sweeps the group and reaps the leader.
pub(super) struct Group {
    child: Option<Child>,
    /// The kernel or another waiter reaped the leader: its id may be reused,
    /// so nothing is signalled any more.
    reaped_elsewhere: bool,
}

/// What the final sweep and reap found.
pub(super) struct Reaped {
    /// The leader's exit status; `None` when it was not reaped within
    /// [`REAP_WAIT`] and is left unreaped.
    pub(super) status: Option<io::Result<ExitStatus>>,
    /// `SIGKILL` reached the group and the leader was reaped.
    pub(super) swept: bool,
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
            // Any other failure is retried at the next poll; every wait on
            // the leader is bounded.
            Err(_) => false,
        }
    }

    /// Wait until the leader exits (true) or `until` passes (false).
    pub(super) fn wait_for_leader(&mut self, until: Instant) -> bool {
        let mut poll = POLL_MIN;
        loop {
            if self.leader_exited() {
                return true;
            }
            let now = Instant::now();
            if now >= until {
                return false;
            }
            thread::sleep(poll.min(until - now));
            poll = (poll * 2).min(POLL_MAX);
        }
    }

    /// Signal the whole group, never after the reap: only while the leader
    /// is held and not reaped elsewhere. Whether the signal was sent.
    pub(super) fn signal(&self, signal: Signal) -> Result<(), Errno> {
        if self.reaped_elsewhere {
            return Err(Errno::CHILD);
        }
        let Some(child) = &self.child else {
            return Err(Errno::CHILD);
        };
        kill_process_group(Pid::from_child(child), signal)
    }

    /// Sweep the group with `SIGKILL`, and the leader by its id too (safe
    /// while it is unreaped), in case it left the group. Then reap the
    /// leader, waiting at most [`REAP_WAIT`]: a leader that is still alive
    /// then (this process may not signal it) is left unreaped rather than
    /// waited for without bound, and stays a zombie once it exits, until the
    /// calling process exits. `None` when an earlier call took it.
    pub(super) fn reap(&mut self) -> Option<Reaped> {
        let leader_had_exited = self.leader_exited();
        // ESRCH once the leader has exited: nothing was left in the group.
        // While the leader was alive, ESRCH means it left the group. EPERM
        // never counts: a member this process may not signal may be alive.
        let group_killed = match self.signal(Signal::KILL) {
            Ok(()) => true,
            Err(Errno::SRCH) => leader_had_exited,
            Err(_) => false,
        };
        if !self.reaped_elsewhere
            && let Some(child) = &self.child
        {
            let _ = kill_process(Pid::from_child(child), Signal::KILL);
        }
        let exited = self.wait_for_leader(Instant::now() + REAP_WAIT);
        let mut child = self.child.take()?;
        if !exited {
            return Some(Reaped {
                status: None,
                swept: false,
            });
        }
        let status = child.wait();
        let swept = group_killed && status.is_ok();
        Some(Reaped {
            status: Some(status),
            swept,
        })
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        let _ = self.reap();
    }
}
