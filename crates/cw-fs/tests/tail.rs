//! Reading the end of a file through the public API, on temporary roots.

use cw_fs::{Cap, append, create_private_dir, read_tail};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn read_tail_returns_the_last_cap_bytes_or_the_whole_shorter_file() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let root = scratch.path().join("data");
    create_private_dir(&root)?;
    let log = root.join("log");
    append(&log, b"first line\nsecond\n")?;

    // The cap falls mid-line: the bytes start there.
    assert_eq!(read_tail(&log, Cap::bytes(9))?, b"e\nsecond\n");
    assert_eq!(
        read_tail(&log, Cap::bytes(1 << 10))?,
        b"first line\nsecond\n"
    );
    Ok(())
}
