//! The process group, the deadline and the sweep, through the public API.
//!
//! Every run has an explicit deadline, and every process a test learns of is
//! killed by a [`Reaper`] on drop, so a failed assertion leaves none behind.

#![cfg(unix)]

mod support;

use cw_supervise::{End, Error, Run};
use std::process::Command;
use std::time::{Duration, Instant};
use support::{Reaper, TestResult, gone_within, pids, sh};

/// How long a killed descendant may take to be reaped by init.
const REAPED_WITHIN: Duration = Duration::from_secs(2);

#[test]
fn the_child_leads_its_own_group_when_the_caller_set_nothing() -> TestResult {
    // Neither a process group nor any stdio is set on this command. `cat`
    // ends at once only if stdin was set to end of file.
    let command = sh("cat; echo $$; ps -o pgid= -p $$", &[]);
    let outcome = Run::new(command, Duration::from_secs(10)).run()?;

    assert!(
        matches!(outcome.end, End::Exited(s) if s.success()),
        "{outcome:?}"
    );
    let ids = pids(&outcome.stdout.bytes);
    assert_eq!(ids.len(), 2, "pid and pgid: {outcome:?}");
    assert_eq!(ids.first(), ids.get(1), "the child is its own group leader");
    assert!(outcome.stdout.complete && outcome.stderr.complete);
    Ok(())
}

#[test]
fn children_that_ignore_sigterm_are_killed_after_the_grace() -> TestResult {
    let mut reaper = Reaper::default();
    // An ignored signal stays ignored across fork and exec, so the
    // background `sleep` ignores SIGTERM too.
    let command = sh(
        "trap '' TERM; sleep 30 & echo $!; echo $$; echo started >&2; wait",
        &[],
    );
    let (deadline, grace) = (Duration::from_millis(500), Duration::from_millis(300));
    let started = Instant::now();
    let outcome = Run::new(command, deadline).grace(grace).run()?;
    let elapsed = started.elapsed();
    let ids = pids(&outcome.stdout.bytes);
    for &pid in &ids {
        reaper.pid(pid);
    }

    assert!(matches!(outcome.end, End::TimedOut), "{outcome:?}");
    assert_eq!(ids.len(), 2, "grandchild and child pids: {outcome:?}");
    assert_eq!(outcome.stderr.bytes, b"started\n", "the output is kept");
    assert!(
        elapsed >= deadline + grace,
        "the whole grace was given: {elapsed:?}"
    );
    assert!(
        elapsed < deadline + grace + Duration::from_secs(2),
        "{elapsed:?}"
    );
    for pid in ids {
        assert!(gone_within(pid, REAPED_WITHIN), "{pid} survived the sweep");
    }
    Ok(())
}

#[test]
fn a_background_grandchild_is_swept_after_a_clean_exit() -> TestResult {
    let mut reaper = Reaper::default();
    let command = sh("sleep 30 >/dev/null 2>&1 & echo $!", &[]);
    let outcome = Run::new(command, Duration::from_secs(10)).run()?;
    let ids = pids(&outcome.stdout.bytes);
    for &pid in &ids {
        reaper.pid(pid);
    }

    assert!(
        matches!(outcome.end, End::Exited(s) if s.success()),
        "{outcome:?}"
    );
    let [grandchild] = ids.as_slice() else {
        return Err(format!("expected one pid: {outcome:?}").into());
    };
    assert!(
        gone_within(*grandchild, REAPED_WITHIN),
        "the grandchild survived"
    );
    Ok(())
}

#[test]
fn a_pipe_held_outside_the_group_is_incomplete_with_the_real_status() -> TestResult {
    let mut reaper = Reaper::default();
    let scratch = tempfile::tempdir()?;
    let pid_file = scratch.path().join("escaped.pid");
    reaper.pid_file(pid_file.clone());
    // perl leaves the group with setsid, records its pid, and keeps the
    // inherited stdout and stderr open. The shell exits 3 once it has.
    let script = r#"
        perl -MPOSIX -e 'POSIX::setsid() or die "setsid: $!"; open(my $f, ">", $ARGV[0]) or die; print $f "$$\n"; close $f; sleep 30' "$1" &
        while [ ! -s "$1" ]; do sleep 0.01; done
        echo before
        exit 3
    "#;
    let path = pid_file.to_str().ok_or("temporary path is not UTF-8")?;
    let started = Instant::now();
    let outcome = Run::new(sh(script, &[path]), Duration::from_secs(20)).run()?;
    let elapsed = started.elapsed();

    assert!(
        matches!(outcome.end, End::Exited(s) if s.code() == Some(3)),
        "the real exit status is kept: {outcome:?}"
    );
    assert_eq!(outcome.stdout.bytes, b"before\n", "what was read is kept");
    assert!(!outcome.stdout.complete, "a held pipe is not complete");
    assert!(!outcome.stderr.complete, "a held pipe is not complete");
    assert!(
        elapsed < Duration::from_secs(6),
        "bounded by the drain wait: {elapsed:?}"
    );
    Ok(())
}

#[test]
fn a_missing_program_is_a_spawn_error_with_not_found() {
    let program = "cw-supervise-test-no-such-program";
    let result = Run::new(Command::new(program), Duration::from_secs(5)).run();

    let Err(error) = result else {
        panic!("a missing program ran: {result:?}");
    };
    match error {
        Error::Spawn {
            program: named,
            source,
        } => {
            assert_eq!(named, program);
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        Error::Unsupported => panic!("unix is supported"),
    }
}
