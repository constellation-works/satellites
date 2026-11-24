//! Files through the public API, on temporary roots.

use cw_fs::{Cap, Error, append, create_new, read, write_atomic};
use std::io;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SIZE: usize = 64 * 1024;

#[test]
fn a_reader_during_write_atomic_sees_the_whole_old_or_the_whole_new_content() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("data");
    write_atomic(&path, &[b'a'; SIZE])?;
    let writer_path = path.clone();
    let writer = std::thread::spawn(move || -> Result<(), Error> {
        for round in 0..200 {
            let byte = if round % 2 == 0 { b'b' } else { b'a' };
            write_atomic(&writer_path, &[byte; SIZE])?;
        }
        Ok(())
    });
    let mut reads = 0;
    while !writer.is_finished() || reads == 0 {
        let bytes = read(&path, Cap::bytes(1 << 20))?;
        assert_eq!(bytes.len(), SIZE, "a torn file was read");
        assert!(
            bytes.iter().all(|b| *b == bytes[0]),
            "a mix of old and new content was read"
        );
        reads += 1;
    }
    writer.join().map_err(|_| "the writer panicked")??;
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
