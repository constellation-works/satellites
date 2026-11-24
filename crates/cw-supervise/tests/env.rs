//! The environment helper, through the public API: what a child actually
//! receives, read back with `env`.

#![cfg(unix)]

mod support;

use cw_supervise::env::{self, Allow};
use cw_supervise::{End, Run};
use std::ffi::OsString;
use std::process::Command;
use std::time::Duration;
use support::TestResult;

/// What `/usr/bin/env` prints when run with `command`'s environment, as
/// sorted lines.
fn child_env(command: Command) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let outcome = Run::new(command, Duration::from_secs(10)).run()?;
    if !matches!(outcome.end, End::Exited(s) if s.success()) {
        return Err(format!("env failed: {outcome:?}").into());
    }
    let mut lines: Vec<String> = String::from_utf8(outcome.stdout.bytes)?
        .lines()
        .map(str::to_owned)
        .collect();
    lines.sort();
    Ok(lines)
}

#[test]
fn allow_passes_only_the_named_variables_that_are_set() -> TestResult {
    let unset = "CW_SUPERVISE_TEST_NEVER_SET";
    assert!(std::env::var_os(unset).is_none(), "{unset} must not be set");
    let path = std::env::var("PATH")?;

    let mut command = Command::new("/usr/bin/env");
    env::allow(&mut command, &["PATH", unset]);

    assert_eq!(child_env(command)?, [format!("PATH={path}")]);
    Ok(())
}

#[test]
fn a_denied_prefix_is_never_passed() -> TestResult {
    let snapshot = [
        ("KEEP", "1"),
        ("ORBIT_TASK", "2"),
        ("TOOL_SECRET", "3"),
        ("TOOL_ROOT", "4"),
        ("ORBIT_TOOL_X", "5"),
        ("OTHER", "6"),
    ]
    .map(|(name, value)| (OsString::from(name), OsString::from(value)));

    let mut command = Command::new("/usr/bin/env");
    Allow::names(["KEEP", "TOOL_SECRET"])
        .prefix("ORBIT_")
        .deny_prefix("TOOL_")
        .apply_from(&mut command, snapshot);

    // TOOL_SECRET is named and still denied; ORBIT_TOOL_X does not start
    // with the denied prefix.
    assert_eq!(
        child_env(command)?,
        ["KEEP=1", "ORBIT_TASK=2", "ORBIT_TOOL_X=5"]
    );
    Ok(())
}
