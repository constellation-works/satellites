//! A private data directory: create it owner-only, take its lock, write a
//! record durably, append to a log, and read both back within a cap.
//!
//! Run with `cargo run -p cw-fs --example private_store`. It works in a
//! temporary directory and removes it on exit.

use cw_fs::{Cap, Lock, append, create_private_dir, list, read, write_atomic};
use std::io::Write as _;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scratch = tempfile::tempdir()?;
    let root = scratch.path().join("store");
    create_private_dir(&root)?;

    // Every writer holds the lock for its whole read-modify-write.
    let lock = Lock::acquire(
        &root.join(".lock"),
        "example writer",
        Duration::from_secs(5),
    )?;
    write_atomic(&root.join("config.json"), br#"{"theme":"dark"}"#)?;
    append(&root.join("audit.jsonl"), b"{\"event\":\"configured\"}\n")?;
    drop(lock);

    let mut out = std::io::stdout().lock();
    for name in list(&root)? {
        let bytes = read(&root.join(&name), Cap::bytes(64 * 1024))?;
        writeln!(out, "{name}: {} bytes", bytes.len())?;
    }
    Ok(())
}
