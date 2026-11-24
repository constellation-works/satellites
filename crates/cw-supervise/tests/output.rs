//! Caps under both policies, the combined log, and input, through the
//! public API. Every run has an explicit deadline; the crate's own guard
//! sweeps what it starts.

#![cfg(unix)]

mod support;

use cw_supervise::{Cap, Captured, End, Outcome, Run};
use std::time::{Duration, Instant};
use support::{TestResult, sh};

/// The deadline for runs that should end long before it.
const DEADLINE: Duration = Duration::from_secs(20);

/// Larger than a pipe buffer (64 KiB on Linux and macOS).
const LARGE: usize = 1 << 20;

fn exited_ok(outcome: &Outcome) -> bool {
    matches!(outcome.end, End::Exited(s) if s.success())
}

fn prefix(captured: &Captured, len: usize, byte: u8) -> bool {
    captured.bytes.len() == len && captured.bytes.iter().all(|&b| b == byte)
}

#[test]
fn truncate_keeps_the_prefix_of_each_stream_and_drains_the_rest() -> TestResult {
    // 256 KiB on each stream: more than a pipe buffer, so a reader that
    // stopped draining would hang the child to its deadline.
    let command = sh(
        "head -c 262144 /dev/zero; head -c 262144 /dev/zero | tr '\\0' e >&2",
        &[],
    );
    let outcome = Run::new(command, DEADLINE)
        .stdout(Cap::truncate(1000))
        .stderr(Cap::truncate(10))
        .run()?;

    assert!(exited_ok(&outcome), "{outcome:?}");
    assert!(
        prefix(&outcome.stdout, 1000, 0),
        "{:?}",
        outcome.stdout.bytes.len()
    );
    assert!(prefix(&outcome.stderr, 10, b'e'), "{:?}", outcome.stderr);
    for captured in [&outcome.stdout, &outcome.stderr] {
        assert!(captured.truncated && captured.complete);
    }
    Ok(())
}

#[test]
fn kill_on_stdout_stops_the_group_and_keeps_the_prefix() -> TestResult {
    let started = Instant::now();
    let outcome = Run::new(sh("yes", &[]), DEADLINE)
        .stdout(Cap::kill(4096))
        .run()?;

    assert!(matches!(outcome.end, End::OutputLimit), "{outcome:?}");
    assert_eq!(outcome.stdout.bytes.len(), 4096);
    assert!(outcome.stdout.bytes.starts_with(b"y\ny\n"));
    assert!(outcome.stdout.truncated);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "stopped at the cap"
    );
    Ok(())
}

#[test]
fn kill_on_stderr_stops_the_group_and_keeps_the_prefix() -> TestResult {
    let started = Instant::now();
    let outcome = Run::new(sh("yes >&2", &[]), DEADLINE)
        .stderr(Cap::kill(100))
        .run()?;

    assert!(matches!(outcome.end, End::OutputLimit), "{outcome:?}");
    assert_eq!(outcome.stderr.bytes.len(), 100);
    assert!(outcome.stderr.bytes.starts_with(b"y\ny\n"));
    assert!(outcome.stderr.truncated);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "stopped at the cap"
    );
    Ok(())
}

#[test]
fn combined_interleaves_both_streams_under_either_policy() -> TestResult {
    let both = Run::new(sh("echo out; echo err >&2; echo out2", &[]), DEADLINE)
        .combined(Cap::truncate(1 << 16))
        .run()?;
    assert!(exited_ok(&both), "{both:?}");
    assert_eq!(both.stdout.bytes, b"out\nerr\nout2\n");
    assert!(both.stdout.complete && !both.stdout.truncated);
    assert!(both.stderr.bytes.is_empty() && both.stderr.complete);

    let cut = Run::new(sh("echo first >&2; head -c 1000 /dev/zero", &[]), DEADLINE)
        .combined(Cap::truncate(8))
        .run()?;
    assert!(exited_ok(&cut), "{cut:?}");
    assert_eq!(cut.stdout.bytes, b"first\n\0\0");
    assert!(cut.stdout.truncated);

    let killed = Run::new(sh("yes >&2", &[]), DEADLINE)
        .combined(Cap::kill(64))
        .run()?;
    assert!(matches!(killed.end, End::OutputLimit), "{killed:?}");
    assert_eq!(killed.stdout.bytes.len(), 64);
    assert!(killed.stdout.truncated);
    Ok(())
}

#[test]
fn input_larger_than_a_pipe_buffer_is_delivered() -> TestResult {
    let input: Vec<u8> = (0..=250u8).cycle().take(LARGE).collect();
    let outcome = Run::new(sh("cat", &[]), DEADLINE)
        .stdin(input.clone())
        .stdout(Cap::truncate(2 * LARGE))
        .run()?;

    assert!(exited_ok(&outcome), "{outcome:?}");
    assert!(!outcome.stdout.truncated && outcome.stdout.complete);
    assert!(outcome.stdout.bytes == input, "the input came back whole");
    Ok(())
}
