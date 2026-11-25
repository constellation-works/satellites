//! The lock wait, by the clock.

use rusqlite::Connection;
use std::time::{Duration, Instant};

/// How long the busy handler sleeps before each retry, by how many times it
/// has already been called for the same lock: short, so that the last sleep
/// ends close to the deadline. Past the end, the last step repeats.
const STEPS_MS: [u64; 5] = [1, 2, 4, 8, 16];

/// The longest lock wait, as SQLite's `busy_timeout` capped it: `i32::MAX`
/// milliseconds, about 24.8 days. A longer wait is cut to this, so no wait
/// is unbounded.
const LONGEST: Duration = Duration::from_millis(i32::MAX as u64);

/// The moment one call stops waiting for another connection: its lock wait
/// after the call started, by [`Instant`]. Every wait in the call that goes
/// through the busy handler shares it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Deadline {
    /// When the call started.
    started: Instant,
    /// The lock wait, at most [`LONGEST`]. What is left is this less the
    /// time elapsed, so no deadline can overflow the clock.
    wait: Duration,
}

impl Deadline {
    /// `wait` from now, at most [`LONGEST`].
    pub(crate) fn after(wait: Duration) -> Self {
        Self {
            started: Instant::now(),
            wait: wait.min(LONGEST),
        }
    }

    /// What is left of the wait; zero once the deadline has passed.
    pub(crate) fn left(self) -> Duration {
        self.wait.saturating_sub(self.started.elapsed())
    }

    /// Whether the deadline has passed.
    pub(crate) fn passed(self) -> bool {
        self.left().is_zero()
    }

    /// Make this deadline `conn`'s busy handler, in place of SQLite's own.
    ///
    /// SQLite's default handler counts its planned sleeps against
    /// `busy_timeout` and never reads the clock, so on a loaded host every
    /// overslept step adds to the wait. This one reads the clock before each
    /// sleep, sleeps no further than the deadline, and gives up once it has
    /// passed. The closure owns the deadline and the connection owns the
    /// closure: `rusqlite` stores it as the connection's client data, which
    /// SQLite frees when the connection closes or the handler is replaced.
    /// Nothing else here sets `busy_timeout`, which would replace it.
    pub(crate) fn install(self, conn: &mut Connection) -> rusqlite::Result<()> {
        conn.busy_handler(Some(move |count: i32| self.wait_once(count)))
    }

    /// One call of the busy handler: sleep a step and retry, or give up.
    fn wait_once(self, count: i32) -> bool {
        let left = self.left();
        if left.is_zero() {
            return false;
        }
        let step = usize::try_from(count)
            .ok()
            .and_then(|i| STEPS_MS.get(i))
            .or(STEPS_MS.last())
            .copied()
            .unwrap_or(1);
        std::thread::sleep(Duration::from_millis(step).min(left));
        true
    }
}
