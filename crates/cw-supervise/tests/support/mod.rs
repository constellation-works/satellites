//! Shared by the run tests: a shell command and a guard that kills the
//! processes a test learns of, so a failed assertion leaves none behind.

#![allow(dead_code, reason = "each test binary uses part of it")]
#![expect(
    clippy::disallowed_methods,
    reason = "support helper: reads a file a child wrote, to compare its bytes"
)]

use rustix::io::Errno;
use rustix::process::{Pid, Signal, kill_process, test_kill_process};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

pub type TestResult = Result<(), Box<dyn std::error::Error>>;

/// `/bin/sh -c <script>`, with any further words as `$1`, `$2`, ….
pub fn sh(script: &str, args: &[&str]) -> Command {
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(script).arg("sh").args(args);
    command
}

/// Kills, on drop, every process id it was told of and not since found gone,
/// and the one written in each pid file it watches, whether the test passed
/// or panicked. Declare it after anything it reads (a pid file's temporary
/// directory), so it drops first.
#[derive(Default)]
pub struct Reaper {
    pids: Vec<i32>,
    pid_files: Vec<PathBuf>,
}

impl Reaper {
    pub fn pid(&mut self, pid: i32) {
        self.pids.push(pid);
    }

    pub fn pid_file(&mut self, path: PathBuf) {
        self.pid_files.push(path);
    }

    /// Whether `pid` is gone within `within`. A pid found gone is forgotten,
    /// so the drop never signals an id that may since have been reused.
    pub fn gone_within(&mut self, pid: i32, within: Duration) -> bool {
        let gone = gone_within(pid, within);
        if gone {
            self.pids.retain(|&p| p != pid);
        }
        gone
    }
}

impl Drop for Reaper {
    fn drop(&mut self) {
        let from_files = self
            .pid_files
            .iter()
            .filter_map(|path| std::fs::read_to_string(path).ok())
            .filter_map(|text| text.trim().parse().ok());
        for pid in self.pids.iter().copied().chain(from_files) {
            if let Some(pid) = Pid::from_raw(pid) {
                let _ = kill_process(pid, Signal::KILL);
            }
        }
    }
}

/// The process ids printed one per line in `text`.
pub fn pids(text: &[u8]) -> Vec<i32> {
    String::from_utf8_lossy(text)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// Whether `pid` is gone within `within`. A killed descendant is reparented
/// and reaped by init, so it may linger briefly as a zombie.
fn gone_within(pid: i32, within: Duration) -> bool {
    let Some(pid) = Pid::from_raw(pid) else {
        return true;
    };
    let until = Instant::now() + within;
    loop {
        if test_kill_process(pid) == Err(Errno::SRCH) {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
