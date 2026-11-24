//! Child processes for the tests that need a second process: this test
//! binary started again on one ignored test, owned so that it is killed and
//! reaped however the parent test ends, and waited on with a deadline.

#![allow(dead_code, reason = "each test file uses its own part of it")]
use std::io::{BufRead as _, BufReader};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

/// How long a parent waits for a child to say it is ready.
pub const READY_WAIT: Duration = Duration::from_secs(10);

/// A child running one ignored test of this binary. Dropping it kills and
/// reaps the process, so a failed assertion or a second spawn that fails
/// leaves nothing behind.
pub struct Owned {
    child: Child,
    lines: Receiver<String>,
}

impl Owned {
    /// Start this test binary on the ignored test `name`, with `env` set.
    pub fn spawn(name: &str, env: &[(&str, &std::ffi::OsStr)]) -> std::io::Result<Self> {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args([
                name,
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command.spawn()?;
        let (send, lines) = channel();
        if let Some(stdout) = child.stdout.take() {
            // Read every line, so the child's last output meets an open pipe.
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let _ = send.send(line);
                }
            });
        }
        Ok(Self { child, lines })
    }

    /// Wait until the child prints a line containing `marker`, or fail at
    /// the deadline or when the child's output ends first.
    pub fn wait_for(&self, marker: &str, deadline: Instant) -> Result<(), String> {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) if line.contains(marker) => return Ok(()),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!("no {marker} by the deadline"));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(format!("the child exited before {marker}"));
                }
            }
        }
    }

    /// The child's stdin, taken: closing it is the child's signal to finish.
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin.take()
    }

    /// Close stdin and wait for the child to exit, killing it at the
    /// deadline. `Ok(true)` when it exited successfully.
    pub fn finish(mut self, deadline: Instant) -> std::io::Result<bool> {
        drop(self.child.stdin.take());
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Ok(status.success());
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

/// Block until stdin closes: a child's way to hold something until told.
pub fn until_stdin_closes() -> std::io::Result<()> {
    let mut line = String::new();
    while std::io::stdin().read_line(&mut line)? > 0 {}
    Ok(())
}
