//! Files through the public API, on temporary roots.

use cw_fs::{Cap, Error, append, create_new, read, write_atomic};
use std::io;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SIZE: usize = 64 * 1024;

/// How many replacements must land while the reader is reading.
const OVERLAP: usize = 20;

/// The most reads the reader makes waiting for them.
const MAX_READS: usize = 1_000_000;

/// The most replacements the writer makes before it stops on its own.
const MAX_WRITES: usize = 100_000;

#[test]
fn a_reader_during_write_atomic_sees_the_whole_old_or_the_whole_new_content() -> TestResult {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let dir = tempfile::tempdir()?;
    let path = dir.path().join("data");
    let old = b"old.".repeat(SIZE / 4);
    let new = b"new!".repeat(SIZE / 4);
    write_atomic(&path, &old)?;

    let stop = Arc::new(AtomicBool::new(false));
    let writes = Arc::new(AtomicUsize::new(0));
    let (started, writing) = std::sync::mpsc::channel();
    let writer = {
        let (path, old, new) = (path.clone(), old.clone(), new.clone());
        let (stop, writes) = (Arc::clone(&stop), Arc::clone(&writes));
        std::thread::spawn(move || -> Result<(), Error> {
            let _ = started.send(());
            for round in 0..MAX_WRITES {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                write_atomic(&path, if round % 2 == 0 { &new } else { &old })?;
                writes.fetch_add(1, Ordering::Release);
            }
            Ok(())
        })
    };
    writing.recv_timeout(std::time::Duration::from_secs(10))?;
    let before = writes.load(Ordering::Acquire);
    let mut reads = 0;
    while writes.load(Ordering::Acquire) - before < OVERLAP && reads < MAX_READS {
        reads += 1;
        let bytes = read(&path, Cap::bytes(1 << 20))?;
        assert!(
            bytes == old || bytes == new,
            "a read saw neither the old nor the new content ({} bytes)",
            bytes.len()
        );
    }
    let during = writes.load(Ordering::Acquire) - before;
    let still_writing = !writer.is_finished();
    stop.store(true, Ordering::Release);
    writer.join().map_err(|_| "the writer panicked")??;
    assert!(
        during >= OVERLAP && still_writing,
        "the reads did not overlap the writes ({during} replacements during {reads} reads)"
    );
    Ok(())
}

#[test]
fn create_new_refuses_an_existing_file_and_leaves_it_alone() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("kept");
    create_new(&path, b"first")?;
    let err = create_new(&path, b"second")
        .err()
        .ok_or("a second create succeeded")?;
    assert!(
        matches!(&err, Error::Io { source, .. } if source.kind() == io::ErrorKind::AlreadyExists),
        "got {err:?}"
    );
    assert_eq!(read(&path, Cap::bytes(64))?, b"first");
    Ok(())
}

#[test]
fn append_adds_records_in_order() -> TestResult {
    let dir = tempfile::tempdir()?;
    let log = dir.path().join("log.jsonl");
    append(&log, b"one\n")?;
    append(&log, b"two\n")?;
    assert_eq!(read(&log, Cap::bytes(64))?, b"one\ntwo\n");
    Ok(())
}

#[test]
fn read_past_the_cap_is_too_large() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("big");
    write_atomic(&path, &[0; 11])?;
    assert_eq!(read(&path, Cap::bytes(11))?.len(), 11);
    let err = read(&path, Cap::bytes(10))
        .err()
        .ok_or("read past the cap")?;
    assert!(
        matches!(err, Error::TooLarge { cap: 10, .. }),
        "got {err:?}"
    );
    Ok(())
}

#[cfg(unix)]
mod unix {
    use super::TestResult;
    use cw_fs::{Cap, EntryKind, Error, append, read};
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn append_creates_the_file_owner_only() -> TestResult {
        let dir = tempfile::tempdir()?;
        let log = dir.path().join("log.jsonl");
        append(&log, b"one\n")?;
        assert_eq!(
            std::fs::metadata(&log)?.permissions().mode() & 0o7777,
            0o600
        );
        Ok(())
    }

    #[test]
    fn read_and_lock_refuse_a_file_open_to_others() -> TestResult {
        use cw_fs::{Lock, write_atomic};
        use std::fs::{Permissions, set_permissions};
        let dir = tempfile::tempdir()?;
        let data = dir.path().join("data");
        write_atomic(&data, b"{}")?;
        set_permissions(&data, Permissions::from_mode(0o644))?;
        let err = read(&data, Cap::bytes(64))
            .err()
            .ok_or("a 0644 file was read")?;
        assert!(
            matches!(
                err,
                Error::Permissions {
                    mode: 0o644,
                    fix: "600",
                    ..
                }
            ),
            "got {err:?}"
        );

        let lock = dir.path().join(".lock");
        drop(Lock::acquire(&lock, "t", Duration::ZERO)?);
        set_permissions(&lock, Permissions::from_mode(0o666))?;
        let err = Lock::acquire(&lock, "t", Duration::ZERO)
            .err()
            .ok_or("a 0666 lock file was taken")?;
        assert!(
            matches!(
                err,
                Error::Permissions {
                    mode: 0o666,
                    fix: "600",
                    ..
                }
            ),
            "got {err:?}"
        );
        Ok(())
    }

    #[test]
    fn read_refuses_a_symlink() -> TestResult {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("target");
        std::fs::write(&target, b"secret")?;
        let link = dir.path().join("link");
        symlink(&target, &link)?;
        let err = read(&link, Cap::bytes(64)).err().ok_or("a link was read")?;
        assert!(matches!(err, Error::Symlink { .. }), "got {err:?}");
        Ok(())
    }

    #[test]
    fn read_refuses_a_fifo_without_blocking() -> TestResult {
        let dir = tempfile::tempdir()?;
        let fifo = dir.path().join("fifo");
        let made = std::process::Command::new("mkfifo").arg(&fifo).status()?;
        assert!(made.success(), "mkfifo failed");
        // A read that blocked would never send; the deadline turns that into
        // a failure instead of a hang.
        let (sent, received) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sent.send(read(&fifo, Cap::bytes(64)));
        });
        let result = received.recv_timeout(Duration::from_secs(10))?;
        let err = result.err().ok_or("a FIFO was read")?;
        assert!(
            matches!(
                err,
                Error::NotRegular {
                    found: EntryKind::Fifo,
                    ..
                }
            ),
            "got {err:?}"
        );
        Ok(())
    }
}
