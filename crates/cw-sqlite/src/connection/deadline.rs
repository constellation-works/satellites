//! The lock wait, by the clock.

use rusqlite::Connection;
use std::time::{Duration, Instant};

/// How long the busy handler sleeps before each retry, by how many times it
/// has already been called for the same lock: short, so that the last sleep
/// ends close to the deadline. Past the end, the last step repeats.
const STEPS_MS: [u64; 5] = [1, 2, 4, 8, 16];

/// The moment one call stops waiting for another connection: its lock wait
/// after the call started, by [`Instant`]. Every wait in the call shares it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Deadline {
    /// `None` when the wait reaches past what an [`Instant`] can hold: the
    /// deadline never passes.
    at: Option<Instant>,
}

impl Deadline {
    /// `wait` from now.
    pub(crate) fn after(wait: Duration) -> Self {
        Self {
            at: Instant::now().checked_add(wait),
        }
    }

    /// What is left of the wait; zero once the deadline has passed.
    pub(crate) fn left(self) -> Duration {
        self.at.map_or(Duration::MAX, |at| {
            at.saturating_duration_since(Instant::now())
        })
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
