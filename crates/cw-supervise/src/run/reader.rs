//! Reading one pipe on its own thread, within a cap.

use super::{Cap, Captured, Overflow};
use std::io::{self, ErrorKind, Read};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Instant;

/// How much one read takes from the pipe.
const CHUNK: usize = 8192;

/// A pipe being read on its own thread. What it read so far is shared, so a
/// reader that never reaches end of file still gives its prefix.
pub(super) struct Reader {
    cap: Cap,
    captured: Arc<Mutex<Captured>>,
    done: Receiver<()>,
}

impl Reader {
    /// Read `pipe` until end of file, keeping at most `cap.limit()` bytes.
    /// Past the cap under [`Overflow::Kill`] it stops and sends on `overflow`.
    pub(super) fn start<R: Read + Send + 'static>(
        mut pipe: R,
        cap: Cap,
        overflow: Sender<()>,
    ) -> io::Result<Self> {
        let captured = Arc::new(Mutex::new(Captured::default()));
        let (done_tx, done) = mpsc::channel();
        let shared = Arc::clone(&captured);
        thread::Builder::new()
            .name(String::from("cw-supervise-reader"))
            .spawn(move || {
                let mut buf = [0u8; CHUNK];
                loop {
                    let n = match pipe.read(&mut buf) {
                        Ok(0) => {
                            lock(&shared).complete = true;
                            break;
                        }
                        Ok(n) => n,
                        Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                        // Output may be missing: `complete` stays false.
                        Err(_) => break,
                    };
                    let mut out = lock(&shared);
                    let room = cap.limit().saturating_sub(out.bytes.len());
                    out.bytes
                        .extend_from_slice(buf.get(..n.min(room)).unwrap_or_default());
                    if n > room {
                        out.truncated = true;
                        if cap.overflow() == Overflow::Kill {
                            out.complete = true;
                            drop(out);
                            let _ = overflow.send(());
                            break;
                        }
                    }
                }
                let _ = done_tx.send(());
            })?;
        Ok(Self {
            cap,
            captured,
            done,
        })
    }

    /// What was read, waiting for the reader until `by`. A reader still
    /// blocked on a held pipe gives its prefix with `complete: false`.
    pub(super) fn collect(self, by: Instant) -> (Cap, Captured) {
        let wait = by.saturating_duration_since(Instant::now());
        // A timeout, or a reader that died, leaves `complete` as it was set.
        let _ = self.done.recv_timeout(wait);
        let captured = std::mem::take(&mut *lock(&self.captured));
        (self.cap, captured)
    }
}

/// The shared capture, even if a reader thread panicked holding it.
fn lock(captured: &Mutex<Captured>) -> std::sync::MutexGuard<'_, Captured> {
    captured.lock().unwrap_or_else(PoisonError::into_inner)
}
