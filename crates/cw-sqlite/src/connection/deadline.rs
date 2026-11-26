//! The lock wait, by the clock.

use rusqlite::Connection;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
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

    /// Sleep one backoff step for `attempt`, ending no later than the deadline.
    /// Returns `false` without sleeping once the deadline has passed.
    pub(crate) fn sleep_retry(self, attempt: u32) -> bool {
        sleep_step(self.left(), usize::try_from(attempt).ok())
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
        sleep_step(self.left(), usize::try_from(count).ok())
    }
}

/// Sleep one step for retry `index`, capped at `left`; returns `false` when
/// `left` is zero.
fn sleep_step(left: Duration, index: Option<usize>) -> bool {
    if left.is_zero() {
        return false;
    }
    let step = index
        .and_then(|i| STEPS_MS.get(i))
        .or(STEPS_MS.last())
        .copied()
        .unwrap_or(1);
    std::thread::sleep(Duration::from_millis(step).min(left));
    true
}

/// The busy handler state for an owned [`super::Writer`]: by default each
/// SQLite lock wait (`count == 0`) gets a fresh clock-bounded `lock_wait`, and
/// a crate call on the writer (such as `checkpoint_truncate`) can pin one
/// shared [`Deadline`] across every lock wait inside that call.
pub(crate) struct DeadlineSlot {
    epoch: Instant,
    /// `0` means no active override (use per-wait `lock_wait`); any non-zero
    /// value `n` encodes a deadline at `epoch + Duration::from_nanos(n - 1)`.
    override_until_ns: Arc<AtomicU64>,
}

impl fmt::Debug for DeadlineSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeadlineSlot").finish_non_exhaustive()
    }
}

impl DeadlineSlot {
    /// Install a per-wait clock-bounded busy handler on `conn` and return the
    /// slot that lets crate calls on `&Writer` temporarily pin a single
    /// [`Deadline`].
    pub(crate) fn install(wait: Duration, conn: &mut Connection) -> rusqlite::Result<Self> {
        let wait = wait.min(LONGEST);
        let epoch = Instant::now();
        let override_until_ns = Arc::new(AtomicU64::new(0));
        let shared = Arc::clone(&override_until_ns);
        let mut started = epoch;
        conn.busy_handler(Some(move |count: i32| {
            let raw = shared.load(Ordering::Relaxed);
            let left = if raw != 0 {
                let until_ns = raw - 1;
                let elapsed_ns = u64::try_from(epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
                Duration::from_nanos(until_ns.saturating_sub(elapsed_ns))
            } else {
                if count == 0 {
                    started = Instant::now();
                }
                wait.saturating_sub(started.elapsed())
            };
            sleep_step(left, usize::try_from(count).ok())
        }))?;
        Ok(Self {
            epoch,
            override_until_ns,
        })
    }

    /// Pin the busy handler to `deadline` until the returned guard drops.
    pub(crate) fn override_with(&self, deadline: Deadline) -> DeadlineOverride<'_> {
        let elapsed_ns = u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX - 1);
        let left_ns = u64::try_from(deadline.left().as_nanos()).unwrap_or(u64::MAX - 1);
        let until_ns = elapsed_ns.saturating_add(left_ns).min(u64::MAX - 1);
        let previous = self.override_until_ns.swap(until_ns + 1, Ordering::Relaxed);
        DeadlineOverride {
            slot: self,
            previous,
        }
    }
}

/// Restores [`DeadlineSlot`] when dropped, on success, error or unwind.
pub(crate) struct DeadlineOverride<'a> {
    slot: &'a DeadlineSlot,
    previous: u64,
}

impl Drop for DeadlineOverride<'_> {
    fn drop(&mut self) {
        self.slot
            .override_until_ns
            .store(self.previous, Ordering::Relaxed);
    }
}
